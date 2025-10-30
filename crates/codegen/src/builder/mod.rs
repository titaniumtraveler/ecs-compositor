use std::path::{Path, PathBuf};

pub use self::dir::Dir;

mod dir;

#[non_exhaustive]
pub struct Wayland {}

impl Wayland {
    pub fn protocols<'a, Iter: IntoIterator<Item = Event<'a>>>(iter: Iter) {
        let mut context = Context::default();

        for event in iter {
            match event {
                Event::EnterDir { in_dir, out_dir } => {
                    if let Some(path) = in_dir {
                        context.in_dir.push(path);
                    }

                    if let Some(path) = out_dir {
                        context.out_dir.push(path);
                    }
                }
                Event::Protocol {
                    in_file,
                    out_file,
                    formatted,
                } => {
                    {
                        context.in_buf.clear();
                        context.in_buf.extend(&context.in_dir);

                        context.in_buf.push(in_file);
                    }

                    {
                        context.out_buf.clear();
                        context.out_buf.extend(&context.out_dir);

                        std::fs::create_dir_all(&context.out_buf).unwrap();

                        context.out_buf.push(out_file);
                    }

                    println!("cargo::rerun-if-changed={}", &context.in_buf.display());
                    crate::protocol(&context.in_buf, &context.out_buf, formatted);
                }
                Event::ExitDir { in_dir, out_dir } => {
                    if in_dir {
                        context.in_dir.pop();
                    }

                    if out_dir {
                        context.out_dir.pop();
                    }
                }
            }
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct Context<'a> {
    in_buf: PathBuf,
    out_buf: PathBuf,

    in_dir: Vec<&'a Path>,
    out_dir: Vec<&'a Path>,
}

#[derive(Debug)]
pub enum Event<'a> {
    EnterDir {
        in_dir: Option<&'a Path>,
        out_dir: Option<&'a Path>,
    },
    Protocol {
        in_file: &'a Path,
        out_file: &'a Path,
        formatted: bool,
    },
    ExitDir {
        in_dir: bool,
        out_dir: bool,
    },
}
