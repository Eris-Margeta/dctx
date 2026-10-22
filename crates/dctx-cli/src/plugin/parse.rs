//! Argument parser for `dctx plugin` and the `dctx git` alias.
//! Implements PLUGINS.md §5.1 command shapes and §3.5 reserved routing.
//! Depends only on std. Used by the plugin entry before any registry or network call.

/// One management or alias invocation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    /// Subcommand.
    pub action: Action,
    /// Bounded JSON on stdout.
    pub json: bool,
}

/// Subcommands from §5.1 plus the git alias.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// `plugin` or `plugin help`.
    Help,
    /// `plugin search [query] [--refresh]`.
    Search {
        /// Empty searches the whole cached index.
        query: String,
        /// Fetch and replace the cached index.
        refresh: bool,
    },
    /// `plugin install`.
    Install(Install),
    /// `plugin list`.
    List,
    /// `plugin info <id>`.
    Info {
        /// Plugin id.
        id: String,
    },
    /// `plugin enable`.
    Enable {
        /// Plugin id.
        id: String,
        /// Project scope, when set.
        project: Option<String>,
        /// Explicit approval of the displayed capability set.
        approve: bool,
    },
    /// `plugin disable`.
    Disable {
        /// Plugin id.
        id: String,
        /// Accepted and reported; the registry disable is user-wide.
        project: Option<String>,
    },
    /// `plugin remove`.
    Remove {
        /// Plugin id.
        id: String,
    },
    /// `plugin update`.
    Update {
        /// Omitted updates the bundled plugin.
        id: Option<String>,
        /// Exact version pin.
        version: Option<String>,
        /// Report a candidate without downloading.
        check: bool,
    },
    /// `plugin verify`.
    Verify {
        /// Omitted verifies the bundled plugin.
        id: Option<String>,
        /// Recheck revocation freshness before artifacts.
        refresh_trust: bool,
    },
    /// `plugin run <id> -- <args>`.
    Run {
        /// Plugin id.
        id: String,
        /// Arguments after `--`.
        args: Vec<String>,
    },
    /// `dctx git <verb> [args]`.
    Git {
        /// Verb and arguments, without a stripped `--json`.
        args: Vec<String>,
    },
}

/// How `plugin install` names the release.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Install {
    /// `owner/repo` or `owner/repo@version` from the verified index.
    Index {
        /// Repository locator.
        repository: String,
        /// Exact version, if `@version` was present.
        version: Option<String>,
        /// Combine with enable.
        enable: bool,
        /// Explicit capability approval. `--enable` requires this flag.
        approve: bool,
    },
    /// `--url` plus the identity the user is approving.
    Url {
        /// Manifest URL.
        url: String,
        /// Expected plugin id.
        id: String,
        /// Combine with enable.
        enable: bool,
        /// Explicit capability approval.
        approve: bool,
    },
    /// `--file` offline bundle.
    File {
        /// Bundle path.
        path: String,
        /// Expected plugin id.
        id: String,
        /// Combine with enable.
        enable: bool,
        /// Explicit capability approval.
        approve: bool,
    },
}

/// Parses `dctx plugin ...` or `dctx git ...`. `args[0]` is the program name.
pub fn parse_argv(args: &[String]) -> Result<Request, String> {
    let rest = args.get(1).map_or("", String::as_str);
    match rest {
        "git" => parse_git(&args[2..]),
        "plugin" => parse_plugin(&args[2..]),
        other => Err(format!("unknown plugin entry: {other}")),
    }
}

fn parse_git(args: &[String]) -> Result<Request, String> {
    let (args, json) = strip_json(args);
    if args.iter().any(|arg| arg == "--") {
        return Err("dctx git does not take a -- separator; use plugin run".to_owned());
    }
    Ok(Request { action: Action::Git { args }, json })
}

fn parse_plugin(args: &[String]) -> Result<Request, String> {
    if args.is_empty() || args[0] == "help" || args[0] == "--help" || args[0] == "-h" {
        let json = args.iter().any(|arg| arg == "--json");
        return Ok(Request { action: Action::Help, json });
    }
    let (args, json) = strip_json(args);
    let action = match args.first().map(String::as_str) {
        Some("search") => parse_search(&args[1..])?,
        Some("install") => Action::Install(parse_install(&args[1..])?),
        Some("list") => only_flags(&args[1..], "list").map(|()| Action::List)?,
        Some("info") => one_positional(&args[1..], "info").map(|id| Action::Info { id })?,
        Some("enable") => parse_switch(&args[1..], true)?,
        Some("disable") => parse_switch(&args[1..], false)?,
        Some("remove") => one_positional(&args[1..], "remove").map(|id| Action::Remove { id })?,
        Some("update") => parse_update(&args[1..])?,
        Some("verify") => parse_verify(&args[1..])?,
        Some("run") => parse_run(&args[1..])?,
        Some(other) => return Err(format!("unknown plugin command: {other}")),
        None => Action::Help,
    };
    Ok(Request { action, json })
}

fn parse_search(args: &[String]) -> Result<Action, String> {
    let mut query = String::new();
    let mut refresh = false;
    for arg in args {
        match arg.as_str() {
            "--refresh" => refresh = true,
            other if other.starts_with('-') => return Err(format!("unknown search flag: {other}")),
            other if query.is_empty() => other.clone_into(&mut query),
            _ => return Err("search accepts one query".to_owned()),
        }
    }
    Ok(Action::Search { query, refresh })
}

fn parse_install(args: &[String]) -> Result<Install, String> {
    let mut url = None;
    let mut file = None;
    let mut id = None;
    let mut locator = None;
    let mut enable = false;
    let mut approve = false;
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        match arg {
            "--enable" => enable = true,
            "--approve" => approve = true,
            "--url" => url = Some(need_value(args, &mut index, "--url")?),
            "--file" => file = Some(need_value(args, &mut index, "--file")?),
            "--id" => id = Some(need_value(args, &mut index, "--id")?),
            other if other.starts_with('-') => return Err(format!("unknown install flag: {other}")),
            other if locator.is_none() => locator = Some(other.to_owned()),
            _ => return Err("install accepts one locator".to_owned()),
        }
        index += 1;
    }
    let sources = u8::from(url.is_some()) + u8::from(file.is_some()) + u8::from(locator.is_some());
    if sources != 1 {
        return Err("install requires one of <owner/repo>, --url or --file".to_owned());
    }
    if enable && !approve {
        return Err("install --enable requires --approve of the displayed capabilities".to_owned());
    }
    if let Some(url) = url {
        return Ok(Install::Url { url, id: id.ok_or("install --url requires --id")?, enable, approve });
    }
    if let Some(path) = file {
        return Ok(Install::File { path, id: id.ok_or("install --file requires --id")?, enable, approve });
    }
    let locator = locator.unwrap_or_default();
    let (repository, version) = split_version(&locator)?;
    Ok(Install::Index { repository, version, enable, approve })
}

fn parse_switch(args: &[String], enable: bool) -> Result<Action, String> {
    let mut id = None;
    let mut project = None;
    let mut approve = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--approve" if enable => approve = true,
            "--approve" => return Err("disable does not accept --approve".to_owned()),
            "--project" => project = Some(need_value(args, &mut index, "--project")?),
            other if other.starts_with('-') => return Err(format!("unknown flag: {other}")),
            other if id.is_none() => id = Some(other.to_owned()),
            _ => return Err("one plugin id expected".to_owned()),
        }
        index += 1;
    }
    let id = id.ok_or("a plugin id is required")?;
    if enable {
        Ok(Action::Enable { id, project, approve })
    } else {
        Ok(Action::Disable { id, project })
    }
}

fn parse_update(args: &[String]) -> Result<Action, String> {
    let mut id = None;
    let mut version = None;
    let mut check = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--check" => check = true,
            "--version" => version = Some(need_value(args, &mut index, "--version")?),
            other if other.starts_with('-') => return Err(format!("unknown update flag: {other}")),
            other if id.is_none() => id = Some(other.to_owned()),
            _ => return Err("update accepts one plugin id".to_owned()),
        }
        index += 1;
    }
    Ok(Action::Update { id, version, check })
}

fn parse_verify(args: &[String]) -> Result<Action, String> {
    let mut id = None;
    let mut refresh_trust = false;
    for arg in args {
        match arg.as_str() {
            "--refresh-trust" => refresh_trust = true,
            other if other.starts_with('-') => return Err(format!("unknown verify flag: {other}")),
            other if id.is_none() => id = Some(other.to_owned()),
            _ => return Err("verify accepts one plugin id".to_owned()),
        }
    }
    Ok(Action::Verify { id, refresh_trust })
}

fn parse_run(args: &[String]) -> Result<Action, String> {
    let Some(split) = args.iter().position(|arg| arg == "--") else {
        return Err("plugin run requires -- before arguments".to_owned());
    };
    let head = &args[..split];
    if head.len() != 1 || head[0].starts_with('-') {
        return Err("plugin run requires <id> -- <args>".to_owned());
    }
    Ok(Action::Run { id: head[0].clone(), args: args[split + 1..].to_vec() })
}

fn split_version(locator: &str) -> Result<(String, Option<String>), String> {
    if locator.starts_with('-') || !locator.contains('/') {
        return Err("install locator must be owner/repo[@version]".to_owned());
    }
    match locator.split_once('@') {
        Some((repository, version)) => {
            if repository.is_empty() || version.is_empty() || repository.contains('@') {
                return Err("install locator must be owner/repo[@version]".to_owned());
            }
            Ok((repository.to_owned(), Some(version.to_owned())))
        }
        None => Ok((locator.to_owned(), None)),
    }
}

fn one_positional(args: &[String], command: &str) -> Result<String, String> {
    if args.len() == 1 && !args[0].starts_with('-') {
        Ok(args[0].clone())
    } else {
        Err(format!("{command} requires one plugin id"))
    }
}

fn only_flags(args: &[String], command: &str) -> Result<(), String> {
    if args.is_empty() { Ok(()) } else { Err(format!("{command} does not take arguments")) }
}

fn need_value(args: &[String], index: &mut usize, flag: &str) -> Result<String, String> {
    *index += 1;
    args.get(*index).filter(|value| !value.starts_with('-')).cloned().ok_or_else(|| format!("{flag} requires a value"))
}

fn strip_json(args: &[String]) -> (Vec<String>, bool) {
    let json = args.iter().any(|arg| arg == "--json");
    let kept = args.iter().filter(|arg| arg.as_str() != "--json").cloned().collect();
    (kept, json)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Request {
        let owned: Vec<String> = std::iter::once("dctx").chain(args.iter().copied()).map(str::to_owned).collect();
        parse_argv(&owned).unwrap()
    }

    #[test]
    fn every_section_5_command_parses_and_unknown_flags_refuse() {
        assert!(matches!(parse(&["plugin"]).action, Action::Help));
        assert!(matches!(parse(&["plugin", "search", "git", "--refresh", "--json"]), Request { json: true, action: Action::Search { refresh: true, .. } }));
        assert!(matches!(parse(&["plugin", "list", "--json"]).action, Action::List));
        assert!(matches!(parse(&["plugin", "info", "dctx/git"]).action, Action::Info { .. }));
        assert!(matches!(parse(&["plugin", "enable", "dctx/git", "--approve", "--project", "/work"]).action, Action::Enable { approve: true, .. }));
        assert!(matches!(parse(&["plugin", "disable", "dctx/git"]).action, Action::Disable { .. }));
        assert!(matches!(parse(&["plugin", "remove", "dctx/git"]).action, Action::Remove { .. }));
        assert!(matches!(parse(&["plugin", "update", "--check"]).action, Action::Update { check: true, .. }));
        assert!(matches!(parse(&["plugin", "verify", "--refresh-trust"]).action, Action::Verify { refresh_trust: true, .. }));
        assert!(matches!(parse(&["plugin", "run", "dctx/git", "--", "status"]).action, Action::Run { .. }));
        assert!(matches!(parse(&["git", "status", "--json"]), Request { json: true, .. }));
        let owned = vec!["dctx".to_owned(), "plugin".to_owned(), "install".to_owned(), "--enable".to_owned()];
        assert!(parse_argv(&owned).is_err());
        let owned = vec!["dctx".to_owned(), "plugin".to_owned(), "search".to_owned(), "--download".to_owned()];
        assert!(parse_argv(&owned).is_err());
    }
}
