use std::cell::RefCell;
use std::io;

use indexmap::map::IndexMap;

#[derive(Clone, PartialEq)]
pub enum Polarity {
    Input,
    Output,
}

#[derive(new, Clone)]
pub struct VPort {
    polarity: Polarity,
    bits: usize,
}

#[derive(Clone, new, PartialEq, Eq, Hash, Debug)]
pub struct VPortLoc {
    // None means this module
    mod_name: Option<String>,
    port_name: String,
}

#[derive(new, Clone)]
pub struct VConn {
    src: VPortLoc,
    dst: VPortLoc,
    bits: usize,
}

#[derive(Clone)]
pub enum VModule {
    External {
        name: String,
        // bitwidth (TODO: support more generic parameters)
        param: usize,
        interfaces: IndexMap<String, VPort>,
    },
    Internal {
        name: String,
        interfaces: IndexMap<String, VPort>,
        internals: IndexMap<String, VModule>,
        connections: Vec<VConn>,
    },
}

impl VModule {
    fn get_name(&self) -> &str {
        match self {
            VModule::External { name, .. } => name,
            VModule::Internal { name, .. } => name,
        }
    }

    fn get_interfaces(&self) -> &IndexMap<String, VPort> {
        match self {
            VModule::External { interfaces, .. } => interfaces,
            VModule::Internal { interfaces, .. } => interfaces,
        }
    }
}

fn generate_wire_name(input: &VPortLoc, output: &VPortLoc) -> String {
    // If one of the port is a module interface, use it.
    if let None = &input.mod_name {
        return input.port_name.clone();
    }
    if let None = &output.mod_name {
        return output.port_name.clone();
    }

    // None cases are covered by above code
    let input_mod_name = input.mod_name.as_ref().unwrap();
    let output_mod_name = output.mod_name.as_ref().unwrap();

    format!(
        "{}_{}_{}_{}",
        input_mod_name, input.port_name, output_mod_name, output.port_name
    )
}

pub fn generate_module_decl<T: io::Write>(vmod: &VModule, defs: &mut T) {
    let cur_tab = RefCell::new(0);

    let open_scope = || {
        *cur_tab.borrow_mut() += 4;
    };

    let close_scope = || {
        *cur_tab.borrow_mut() -= 4;
    };

    macro_rules! gen {
        ( $stream:expr, $( $e:expr ),* ) => {
            for _ in 0..*cur_tab.borrow() {
                $stream.write(" ".as_bytes()).unwrap();
            }
            $stream.write(format!( $( $e ),* ).as_bytes()).unwrap();
        };
    }
    macro_rules! genln {
        ( $stream:expr, $( $e:expr ),* ) => {
            gen!($stream, $($e),*);
            $stream.write("\n".as_bytes()).unwrap();
        };
    }

    match vmod {
        VModule::External { .. } => panic!("generate_module_decl accepts only Internal module"),
        VModule::Internal {
            name,
            interfaces,
            internals,
            connections,
        } => {
            genln!(defs, "module {} (", name);
            {
                open_scope();
                let args = interfaces
                    .iter()
                    .map(|(name, _)| name.clone())
                    .collect::<Vec<_>>()
                    .join(", ");
                genln!(defs, "{}", args);
                close_scope();
            }
            genln!(defs, ");");

            {
                open_scope();

                // Generate port decls
                for (name, port) in interfaces {
                    let io = if port.polarity == Polarity::Input {
                        "input"
                    } else {
                        "output"
                    };

                    let bitwidth = &if port.bits > 1 {
                        format!("[{}:0]", port.bits - 1)
                    } else {
                        "".to_string()
                    };

                    genln!(defs, "{} {} {};", io, bitwidth, name);
                }

                // Create wire
                // Wires
                let mut wires = Vec::<(String, usize)>::new();
                // Module name & port name -> wire name & bitwidth
                let mut port_to_wire = IndexMap::<VPortLoc, (String, usize)>::new();
                // assign dst = src;
                let mut assigns = Vec::<(String, String)>::new();

                for VConn { src, dst, bits } in connections {
                    match (&src.mod_name, &dst.mod_name) {
                        (None, None) => {
                            // interface = interface
                            assigns.push((dst.port_name.clone(), src.port_name.clone()))
                        }
                        (Some(mod_name), None) => {
                            // interface = internal module port
                            let wire_name = format!("{}_{}", mod_name, src.port_name.clone());
                            wires.push((wire_name.clone(), *bits));
                            port_to_wire.insert(src.clone(), (wire_name.clone(), *bits));
                            assigns.push((dst.port_name.clone(), wire_name.clone()))
                        }
                        (None, Some(mod_name)) => {
                            // internal module port = interface
                            let wire_name = format!("{}_{}", mod_name, dst.port_name.clone());
                            wires.push((wire_name.clone(), *bits));
                            port_to_wire.insert(dst.clone(), (wire_name.clone(), *bits));
                            assigns.push((wire_name.clone(), src.port_name.clone()));
                        }
                        (Some(src_mod_name), Some(dst_mod_name)) => {
                            let src_wire_name =
                                format!("{}_{}", src_mod_name, src.port_name.clone());
                            let dst_wire_name =
                                format!("{}_{}", dst_mod_name, dst.port_name.clone());

                            wires.push((src_wire_name.clone(), *bits));
                            wires.push((src_wire_name.clone(), *bits));

                            port_to_wire.insert(src.clone(), (src_wire_name.clone(), *bits));
                            port_to_wire.insert(dst.clone(), (dst_wire_name.clone(), *bits));

                            assigns.push((dst_wire_name.clone(), src_wire_name.clone()));
                        }
                    }
                }

                for (wire_name, bits) in &wires {
                    let bitwidth = &if *bits > 1 {
                        format!("[{}:0]", *bits - 1)
                    } else {
                        "".to_string()
                    };

                    genln!(defs, "wire {} {};", bitwidth, wire_name);
                }

                for (src, dst) in &assigns {
                    genln!(defs, "assign {} = {};", src, dst);
                }

                for (name, vmod) in internals {
                    let mod_name = vmod.get_name();

                    let args = vmod
                        .get_interfaces()
                        .iter()
                        .map(|(port_name, _)| {
                            let loc = VPortLoc::new(Some(name.clone()), port_name.clone());
                            let (wire_name, _) = port_to_wire
                                .get(&loc)
                                .expect(&format!("The port loc {:?} is not found", loc));
                            wire_name.clone()
                        })
                        .collect::<Vec<_>>()
                        .join(", ");

                    genln!(defs, "{} {} ({}));\n", mod_name, name, args);
                }

                close_scope();
            }

            genln!(defs, "endmodule");
        }
    }
}

mod test_verilog_ir {
    use super::*;

    fn s<T: ToString>(s: T) -> String {
        s.to_string()
    }

    #[test]
    fn test_seq() {
        let d_flip_flop = VModule::External {
            name: s("d_flip_flop"),
            param: 8,
            interfaces: vec![
                (s("in"), VPort::new(Polarity::Input, 8)),
                (s("out"), VPort::new(Polarity::Output, 8)),
            ]
            .into_iter()
            .collect(),
        };

        // seq: con * exp -> exp
        // [| con |] = (+0, -0)
        // [| exp |] = (+0, -n)
        // [| con * exp -> exp |] = (-0, +0, -0, +n, +0, -n)
        let vmod = VModule::Internal {
            name: s("seq"),
            interfaces: vec![
                (s("cmd_req"), VPort::new(Polarity::Output, 1)),
                (s("cmd_valid"), VPort::new(Polarity::Input, 1)),
                (s("exp_req"), VPort::new(Polarity::Output, 1)),
                (s("exp"), VPort::new(Polarity::Input, 8)),
                (s("exp_valid"), VPort::new(Polarity::Input, 1)),
                (s("req"), VPort::new(Polarity::Input, 1)),
                (s("ret"), VPort::new(Polarity::Output, 8)),
                (s("valid"), VPort::new(Polarity::Output, 1)),
            ]
            .into_iter()
            .collect(),

            internals: [("D".to_string(), d_flip_flop)].iter().cloned().collect(),

            connections: vec![
                VConn::new(
                    VPortLoc::new(None, s("req")),
                    VPortLoc::new(None, s("cmd_req")),
                    1,
                ),
                VConn::new(
                    VPortLoc::new(None, s("cmd_valid")),
                    VPortLoc::new(Some(s("D")), s("in")),
                    1,
                ),
                VConn::new(
                    VPortLoc::new(Some(s("D")), s("out")),
                    VPortLoc::new(None, s("exp_req")),
                    1,
                ),
                VConn::new(
                    VPortLoc::new(None, s("exp_valid")),
                    VPortLoc::new(None, s("valid")),
                    8,
                ),
                VConn::new(
                    VPortLoc::new(None, s("exp")),
                    VPortLoc::new(None, s("ret")),
                    8,
                ),
            ],
        };

        let mut buf = Vec::<u8>::new();
        generate_module_decl(&vmod, &mut buf);
        let s = buf.iter().map(|&u| u as char).collect::<String>();
        println!("Generated verilog:\n{}", s)
    }
}
