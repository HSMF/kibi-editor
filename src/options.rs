#[derive(Debug, PartialEq, Eq, Default)]
pub struct Options {
    pub file: Option<String>,
    pub print_supported_keymaps: bool,
    pub debug: bool,
    pub version: bool,
    pub commands: Vec<String>,
}

const DEFAULT_DEBUG: bool = cfg!(debug_assertions);

fn print_help() -> ! {
    eprintln!("Usage:");
    eprintln!("  {} [options] [file]", env!("CARGO_PKG_NAME"));
    eprintln!(
        "  --print-supported-keymaps    print list of all currently supported keymaps and exit",
    );
    fn default(b: bool) -> &'static str {
        if b { " (default)" } else { "" }
    }
    eprintln!(
        "  --debug                      write debug log{}",
        default(DEFAULT_DEBUG)
    );
    eprintln!(
        "  --no-debug                   don't write debug log{}",
        default(!DEFAULT_DEBUG)
    );
    eprintln!("  --version                    print version information and exit",);
    eprintln!("  +<cmd>, -c <cmd>             execute <cmd> before interactive editing",);
    eprintln!("  --                           only file name after this",);
    std::process::exit(0)
}

impl Options {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Self {
        let mut args = args.into_iter();
        args.next();

        let mut ret = Self {
            file: None,
            version: false,
            print_supported_keymaps: false,
            commands: vec![],
            debug: DEFAULT_DEBUG,
        };

        while let Some(arg) = args.next() {
            if arg == "--" {
                break;
            } else if let Some(long_cmd) = arg.strip_prefix("--") {
                match long_cmd {
                    "print-supported-keymaps" => ret.print_supported_keymaps = true,
                    "debug" => ret.debug = true,
                    "no-debug" => ret.debug = false,
                    "help" => print_help(),
                    "version" => ret.version = true,
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

        for arg in args {
            ret.file = Some(arg)
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
                ..Default::default()
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
                commands: vec!["set number".to_owned()],
                ..Default::default()
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
                commands: vec!["set number".to_owned()],
                ..Default::default()
            }
        );
    }

    #[test]
    fn literal_file() {
        let opts = parse(["--", "-c"]);
        assert_eq!(
            opts,
            Options {
                file: Some("-c".to_owned()),
                ..Default::default()
            }
        );
    }
}
