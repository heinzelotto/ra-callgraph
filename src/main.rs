use anyhow::{Context, Result};
use clap::Parser;

use ide_db::base_db::SourceDatabaseExt;
use ide_db::symbol_index::SymbolsDatabase;

mod opts;

mod loader;

#[derive(Copy, Clone, Debug)]
struct Function {
    pos: ide::FilePosition,
    // full_path: Option<Vec<ide::TextRange>>,
}

impl Function {
    fn new(file_id: ide::FileId, navigation_range: ide::TextRange) -> Self {
        Function {
            pos: ide::FilePosition {
                file_id,
                offset: navigation_range.start(),
            },
        }
    }

    fn id_string(&self) -> String {
        format!("{}:{}", self.pos.file_id.0, usize::from(self.pos.offset))
    }
}

impl std::hash::Hash for Function {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.pos.file_id.0.hash(state);
        usize::from(self.pos.offset).hash(state);
    }
}

impl std::cmp::PartialEq for Function {
    fn eq(&self, other: &Self) -> bool {
        self.pos.file_id.0 == other.pos.file_id.0
            && usize::from(self.pos.offset) == usize::from(other.pos.offset)
    }
}
impl std::cmp::Eq for Function {}

fn extract_label(
    _file_id: ide::FileId,
    symbol: &ide::StructureNode,
    file_structure: &Vec<ide::StructureNode>,
) -> String {
    let mut v = vec![symbol.label.clone()];
    let mut n = symbol;
    while let Some(i) = n.parent {
        n = &file_structure[i];
        v.push(n.label.clone());
    }
    v.reverse();
    let path = v.join("::");
    // format!("{}/{}", file_id.0, path)
    format!("{}", path)
}

fn analyze_file(
    all_functions: &mut std::collections::HashSet<Function>,
    function_adjacency: &mut std::collections::HashMap<Function, Vec<Function>>,
    trait_impl_adjacency: &mut std::collections::HashMap<Function, Vec<Function>>,
    function_labels: &mut std::collections::HashMap<Function, String>,
    file_id: ide::FileId,
    analysis: &ide::Analysis,
) -> anyhow::Result<()> {
    analysis.parse(file_id)?;
    let file_structure = analysis.file_structure(file_id)?;

    for symbol in &file_structure {
        if symbol.kind == ide::StructureNodeKind::SymbolKind(ide::SymbolKind::Function) {
            let cur_fun = Function::new(file_id, symbol.navigation_range);

            function_labels.insert(
                cur_fun,
                extract_label(cur_fun.pos.file_id, &symbol, &file_structure),
            );

            if !all_functions.contains(&cur_fun) {
                all_functions.insert(cur_fun);
            }

            let incoming_calls = analysis.incoming_calls(cur_fun.pos)?.unwrap();
            for call_item in incoming_calls {
                let target_fun = Function::new(
                    call_item.target.file_id,
                    call_item.target.focus_range.unwrap(),
                );
                // dbg!(&call_item);

                if call_item
                    .ranges
                    .iter()
                    .all(|r| usize::from(r.start()) == usize::from(target_fun.pos.offset))
                {
                    continue;
                }
                function_adjacency
                    .entry(target_fun)
                    .or_default()
                    .push(cur_fun);
            }

            if let Some(pidx) = symbol.parent {
                if file_structure[pidx].kind
                    == ide::StructureNodeKind::SymbolKind(ide::SymbolKind::Trait)
                {
                    for trait_function_impl in
                        analysis.goto_implementation(cur_fun.pos)?.unwrap().info
                    {
                        let target_fun = Function::new(
                            trait_function_impl.file_id,
                            trait_function_impl.focus_range.unwrap(),
                        );

                        trait_impl_adjacency
                            .entry(cur_fun)
                            .or_default()
                            .push(target_fun);
                    }
                }
            }

            // let outgoing_calls = analysis.outgoing_calls(cur_fun.pos)?.unwrap();
            // for call_item in outgoing_calls {
            //     let target_fun = Function::new(
            //         call_item.target.file_id,
            //         call_item.target.focus_range.unwrap(),
            //     );

            //     function_adjacency
            //         .entry(cur_fun)
            //         .or_default()
            //         .push(target_fun);
            // }

            // let outgoing_calls = analysis.outgoing_calls(fn_file_position)?;
            // dbg!(outgoing_calls);
        }
    }

    Ok(())
}

fn main() -> Result<()> {
    let cmd_line_opts = opts::CmdLineOpts::parse();

    let contents = std::fs::read_to_string(&cmd_line_opts.target)
        .context(format!("Trying to open {}", &cmd_line_opts.target))?;

    let mut all_functions = std::collections::HashSet::new();

    let mut function_adjacency: std::collections::HashMap<Function, Vec<Function>> =
        std::collections::HashMap::new();

    let mut trait_impl_adjacency: std::collections::HashMap<Function, Vec<Function>> =
        std::collections::HashMap::new();

    let mut function_labels: std::collections::HashMap<Function, String> =
        std::collections::HashMap::new();

    let (host, _vfs, _pms) = loader::load_it(std::path::Path::new(&cmd_line_opts.target));

    let analysis = host.analysis();
    for r in host.raw_database().local_roots().iter() {
        for file_id in host.raw_database().source_root(*r).iter() {
            dbg!(file_id);

            analyze_file(
                &mut all_functions,
                &mut function_adjacency,
                &mut trait_impl_adjacency,
                &mut function_labels,
                file_id,
                &analysis,
            )?;
        }
    }

    // let (analysis, file_id) = ide::Analysis::from_single_file(contents);

    //         analyze_file(
    //             &mut all_functions,
    //             &mut function_adjacency,
    //             &mut trait_impl_adjacency,
    //             &mut function_labels,
    //             file_id,
    //             &analysis
    //         )?;

    let node_ids = {
        let mut m = std::collections::HashMap::new();
        let mut idx = 0;
        for k in all_functions.iter() {
            m.insert(k, idx);
            idx += 1;
        }
        m
    };

    let blacklisted_tests: std::collections::HashSet<Function> = all_functions
        .iter()
        .filter(|f| function_labels[f].starts_with("test::"))
        .cloned()
        .collect();

    println!("digraph example {{");
    for f in all_functions.iter() {
        if blacklisted_tests.contains(f) {
            continue;
        }
        println!("{}[label=\"{}\"]", node_ids[f], function_labels[f]);
    }

    for (f, v) in function_adjacency.iter() {
        for t in v {
            if blacklisted_tests.contains(f) || blacklisted_tests.contains(t) {
                continue;
            }
            println!("{} -> {}[label=\"\"]", node_ids[&f], node_ids[&t]);
        }
    }

    for (tr, v) in trait_impl_adjacency.iter() {
        for im in v {
            if blacklisted_tests.contains(tr) || blacklisted_tests.contains(im) {
                continue;
            }
            println!("{} -> {}[label=\"\", arrowhead=\"diamond\", style=\"dashed\", color=\"deeppink1\"]", node_ids[&tr], node_ids[&im]);
        }
    }
    println!("}}");

    Ok(())
}

// plan:
//
// functions are identified by their navigation_range, i. e. their identifier, and the file_id
//
// the navigation range coincides with the focus_range of the CallItem
//
// for trait functions: find all implementations
//
// for functions inside trait impl blocks: filter calls against the textrange

// possibly analysis.find_all_methods, does it also return uncalled method identifiers?
