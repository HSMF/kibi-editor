#[derive(Debug, PartialEq, Eq)]
pub struct Options {
    pub file: Option<String>,
    pub print_supported_keymaps: bool,
    pub commands: Vec<String>,
}

impl Options {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Self {
        let mut args = args.into_iter();
        args.next();

        let mut ret = Self {
            file: None,
            print_supported_keymaps: false,
            commands: vec![],
        };

        while let Some(arg) = args.next() {
            if let Some(long_cmd) = arg.strip_prefix("--") {
                match long_cmd {
                    "print-supported-keymaps" => ret.print_supported_keymaps = true,
                    _ => panic!("unknown option --{long_cmd:?}"),
                }
            } else if arg == "-c" {
                let command = args.next().expect("-c requires an argument");
                ret.commands.push(command.to_owned());
            } else if let Some(command) = arg.strip_prefix("-c") {
                ret.commands.push(command.to_owned());
            } else if let Some(command) = arg.strip_prefix("+") {
                ret.commands.push(command.to_owned());
            } else {
                ret.file = Some(arg)
            }
        }
        ret
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse<'a>(s: impl IntoIterator<Item = &'a str>) -> Options {
        Options::parse(std::iter::once("kibi").chain(s).map(|x| x.to_string()))
    }

    #[test]
    fn empty() {
        let opts = parse([]);
        assert_eq!(
            opts,
            Options {
                file: None,
                print_supported_keymaps: false,
                commands: vec![]
            }
        );
    }

    #[test]
    fn print_supported_keymaps() {
        let opts = parse(["--print-supported-keymaps"]);
        assert!(opts.print_supported_keymaps);
    }

    #[test]
    fn file() {
        let opts = parse(["hello"]);
        assert_eq!(opts.file, Some("hello".to_owned()));
    }

    #[test]
    fn commands() {
        let opts = parse(["hello", "+set number"]);
        assert_eq!(
            opts,
            Options {
                file: Some("hello".to_owned()),
                print_supported_keymaps: false,
                commands: vec!["set number".to_owned()]
            }
        );
    }

    #[test]
    fn commands_c() {
        let opts = parse(["-c", "set number", "hello"]);
        assert_eq!(
            opts,
            Options {
                file: Some("hello".to_owned()),
                print_supported_keymaps: false,
                commands: vec!["set number".to_owned()]
            }
        );
    }
}
