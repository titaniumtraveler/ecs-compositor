use crate::builder::Event;
use std::path::Path;

#[derive(Default)]
pub struct Dir<'a> {
    in_dir: Option<&'a Path>,
    out_dir: Option<&'a Path>,

    children: Vec<Child<'a>>,
}

impl<'a> Dir<'a> {
    pub fn new() -> Self {
        Self {
            in_dir: None,
            out_dir: None,
            children: Vec::default(),
        }
    }

    pub fn in_dir(mut self, path: &'a (impl AsRef<Path> + ?Sized)) -> Self {
        self.in_dir = Some(path.as_ref());
        self
    }

    pub fn out_dir(mut self, path: &'a (impl AsRef<Path> + ?Sized)) -> Self {
        self.out_dir = Some(path.as_ref());
        self
    }

    pub fn protocol(
        mut self,
        in_file: &'a (impl AsRef<Path> + ?Sized),
        out_file: &'a (impl AsRef<Path> + ?Sized),
    ) -> Self {
        self.children.push(Child::Proto(Protocol {
            in_file: in_file.as_ref(),
            out_file: out_file.as_ref(),
            formatted: true,
        }));
        self
    }

    pub fn protocols(
        mut self,
        paths: impl IntoIterator<
            Item = (
                &'a (impl AsRef<Path> + ?Sized + 'a),
                &'a (impl AsRef<Path> + ?Sized + 'a),
            ),
        >,
    ) -> Self {
        self.children
            .extend(paths.into_iter().map(|(in_file, out_file)| {
                Child::Proto(Protocol {
                    in_file: in_file.as_ref(),
                    out_file: out_file.as_ref(),
                    formatted: true,
                })
            }));
        self
    }

    pub fn dir(mut self, dir: Dir<'a>) -> Self {
        self.children.push(Child::Dir(dir));
        self
    }
}

enum Child<'a> {
    Dir(Dir<'a>),
    Proto(Protocol<'a>),
}

struct Protocol<'a> {
    in_file: &'a Path,
    out_file: &'a Path,
    formatted: bool,
}

pub struct IntoIter<'a> {
    first: bool,
    stack: Vec<Dir<'a>>,
}

impl<'a> IntoIterator for Dir<'a> {
    type Item = Event<'a>;
    type IntoIter = IntoIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        IntoIter {
            stack: vec![self],
            first: true,
        }
    }
}

impl<'a> Iterator for IntoIter<'a> {
    type Item = Event<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let Dir {
            in_dir,
            out_dir,
            children,
        } = self.stack.last_mut()?;

        if self.first {
            self.first = false;
            return Some(Event::EnterDir {
                in_dir: *in_dir,
                out_dir: *out_dir,
            });
        }

        match children.pop() {
            Some(Child::Dir(dir)) => {
                let event = Event::EnterDir {
                    in_dir: dir.in_dir,
                    out_dir: dir.out_dir,
                };
                self.stack.push(dir);

                Some(event)
            }

            Some(Child::Proto(Protocol {
                in_file,
                out_file,
                formatted,
            })) => Some(Event::Protocol {
                in_file,
                out_file,
                formatted,
            }),

            None => {
                let Dir {
                    in_dir, out_dir, ..
                } = self.stack.pop().expect("");
                Some(Event::ExitDir {
                    in_dir: in_dir.is_some(),
                    out_dir: out_dir.is_some(),
                })
            }
        }
    }
}
