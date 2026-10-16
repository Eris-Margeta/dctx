//! Read-only code traversal mode of the existing dctx CLI.
//! Implements SPEC-MAP-MODE sections 4 and 9.1 through dctx-map-query.
//! Selects an external cache and prints one bounded answer without snapshot side effects.
mod args;
mod storage;
use dctx_map_query::{render_answer, FileSource, Index};

pub(crate) fn run(arguments: &[String]) -> Result<(), String> {
    if arguments.is_empty() || arguments == ["--help"] || arguments == ["-h"] {
        print!("{}", args::HELP);
        return Ok(());
    }
    let mut options = args::parse(arguments)?;
    if let Ok(focus) = std::env::var("DCTX_MAP_FOCUS") {
        options.request.focus = focus
            .split([',', '\n'])
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(str::to_owned)
            .collect();
        options.request.validate().map_err(|e| e.to_string())?;
    }
    let source = FileSource::new(&options.place).map_err(|e| e.to_string())?;
    let cache = storage::Cache::open(source.root())?;
    let place = source
        .root()
        .file_name()
        .and_then(|p| p.to_str())
        .unwrap_or("place");
    let mut index = Index::open(place, &cache.database).map_err(|e| e.to_string())?;
    let answer = index
        .query(&source, &options.request)
        .map_err(|e| e.to_string())?;
    let mut output =
        render_answer(&answer, &options.request, options.format).map_err(|e| e.to_string())?;
    // Continuations retain the caller's place even when invoked from elsewhere.
    if options.place != std::path::Path::new(".") {
        output = storage::with_place(&answer, &options.request, options.format, &options.place)?;
    }
    index.save(&cache.database).map_err(|e| e.to_string())?;
    print!("{output}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn admin_commands_are_refused_before_any_cache_write() {
        assert!(run(&["index".into()]).is_err());
        assert!(run(&["stats".into()]).is_err());
        assert!(run(&["summaries".into()]).is_err());
    }
}
