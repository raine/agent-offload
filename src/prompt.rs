use anyhow::{Context, Result, bail};
use std::io::{self, IsTerminal, Read};
use std::path::Path;

pub fn load_prompt(args: &[String]) -> Result<String> {
    if !args.is_empty() {
        return Ok(args.join(" "));
    }

    if io::stdin().is_terminal() {
        bail!("provide a prompt argument or pipe prompt text on stdin");
    }

    let mut prompt = String::new();
    io::stdin()
        .read_to_string(&mut prompt)
        .context("could not read prompt from stdin")?;

    let prompt = prompt.trim().to_string();
    if prompt.is_empty() {
        bail!("prompt from stdin is empty");
    }

    Ok(prompt)
}

pub fn augment_prompt(prompt: &str, done_file: &Path) -> String {
    let tmp_file = done_file.with_extension("md.tmp");
    format!(
        r#"{prompt}

When the implementation is complete, write a short summary to this temporary file:

{tmp_file}

Then atomically rename it to this final completion file:

{done_file}

The launcher is waiting for the final completion file to exist. Do not create the
final file until you are done with the delegated work. Keep the summary concise
and include files changed, checks run, and any unresolved issues.
"#,
        prompt = prompt,
        tmp_file = tmp_file.display(),
        done_file = done_file.display(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_augment_prompt_includes_paths() {
        let done = PathBuf::from("/tmp/test-run/done.md");
        let result = augment_prompt("implement the feature", &done);

        assert!(result.contains("implement the feature"));
        assert!(result.contains("/tmp/test-run/done.md.tmp"));
        assert!(result.contains("/tmp/test-run/done.md"));
        assert!(result.contains("atomically rename"));
    }

    #[test]
    fn test_load_prompt_from_args() {
        let args = vec!["implement".to_string(), "feature".to_string()];
        assert_eq!(load_prompt(&args).unwrap(), "implement feature");
    }

    #[test]
    fn test_load_prompt_empty_args_fails_when_terminal() {
        // This would bail on is_terminal() - unit-test the terminal path separately
        let args: Vec<String> = vec![];
        let result = load_prompt(&args);
        assert!(result.is_err());
    }
}
