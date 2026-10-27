//! Inputs: values a command asks for each time it runs. The command names
//! each one with a variable that starts with `INPUT_`, written the way its
//! shell writes variables, and the value reaches it as an environment
//! variable. The shell never reads the typed text as code, so quotes,
//! backticks, `$` and new lines arrive exactly as typed.

pub const PREFIX: &str = "INPUT_";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Input {
    /// The variable, such as `INPUT_RELEASE_NOTES`.
    pub var: String,
    /// What the run sheet calls it, such as "Release notes".
    pub label: String,
}

/// The inputs `line` uses, in the order they first appear, each once:
/// `$INPUT_X` and `${INPUT_X}` as sh writes them, `$env:INPUT_X` and
/// `${env:INPUT_X}` as PowerShell does.
pub fn inputs(line: &str) -> Vec<Input> {
    let mut found: Vec<Input> = Vec::new();
    let mut rest = line;
    while let Some(at) = rest.find('$') {
        rest = &rest[at + 1..];
        let named = without_env(rest.strip_prefix('{').unwrap_or(rest));
        let Some(name) = named.strip_prefix(PREFIX) else {
            continue;
        };
        let end = name
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(name.len());
        let name = &name[..end];
        // `$INPUT_` alone, or underscores only, names nothing to ask for.
        if !name.chars().any(|c| c.is_ascii_alphanumeric()) {
            continue;
        }
        let var = format!("{PREFIX}{name}");
        if !found.iter().any(|i| i.var == var) {
            found.push(Input {
                label: label(name),
                var,
            });
        }
    }
    found
}

/// The environment for a run of `line`: the given value of each of its
/// inputs, or the first input that has none.
pub fn values_for(line: &str, given: &[(String, String)]) -> Result<Vec<(String, String)>, Input> {
    inputs(line)
        .into_iter()
        .map(|i| match given.iter().find(|(var, _)| *var == i.var) {
            Some((_, value)) => Ok((i.var, value.clone())),
            None => Err(i),
        })
        .collect()
}

/// PowerShell's `env:` drive, which it reads in any case.
fn without_env(s: &str) -> &str {
    match s.get(..4) {
        Some(drive) if drive.eq_ignore_ascii_case("env:") => &s[4..],
        _ => s,
    }
}

/// `RELEASE_NOTES` reads "Release notes".
fn label(name: &str) -> String {
    let words = name
        .split('_')
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let mut chars = words.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => words,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(line: &str) -> Vec<String> {
        inputs(line).into_iter().map(|i| i.var).collect()
    }

    #[test]
    fn finds_the_sh_and_powershell_forms() {
        assert_eq!(
            vars("x $INPUT_A ${INPUT_B} $env:INPUT_C ${env:INPUT_D} $Env:INPUT_E"),
            ["INPUT_A", "INPUT_B", "INPUT_C", "INPUT_D", "INPUT_E"]
        );
    }

    #[test]
    fn lists_each_once_in_order_of_first_use() {
        assert_eq!(
            vars("$INPUT_B; echo \"$INPUT_A\" ${INPUT_B}"),
            ["INPUT_B", "INPUT_A"]
        );
    }

    #[test]
    fn a_name_ends_where_its_characters_do() {
        assert_eq!(
            vars("\"$INPUT_NOTES\"x ${INPUT_V:-1} $INPUT_A.txt $INPUT_2nd"),
            ["INPUT_NOTES", "INPUT_V", "INPUT_A", "INPUT_2nd"]
        );
    }

    #[test]
    fn other_variables_are_not_inputs() {
        assert!(
            vars("$HOME $INPUTX $INPUT_ $INPUT__ ${HOME} $env:PATH $(INPUT_X) INPUT_Y $input_z")
                .is_empty()
        );
    }

    #[test]
    fn labels_read_as_words() {
        let labels: Vec<String> = inputs("$INPUT_RELEASE_NOTES $INPUT_VERSION $INPUT_api__key_")
            .into_iter()
            .map(|i| i.label)
            .collect();
        assert_eq!(labels, ["Release notes", "Version", "Api key"]);
    }

    #[test]
    fn a_run_needs_a_value_for_every_input() {
        let line = "./release.sh \"$INPUT_VERSION\" --notes \"$INPUT_NOTES\"";
        let given = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        };
        let missing = values_for(line, &given(&[("INPUT_NOTES", "Fixes")])).unwrap_err();
        assert_eq!(missing.label, "Version");
        // Values for what the text no longer uses are left out; an empty
        // value is still a value.
        let env = values_for(
            line,
            &given(&[
                ("INPUT_OLD", "x"),
                ("INPUT_NOTES", ""),
                ("INPUT_VERSION", "2"),
            ]),
        )
        .unwrap();
        assert_eq!(env, given(&[("INPUT_VERSION", "2"), ("INPUT_NOTES", "")]));
        assert_eq!(values_for("make", &[]).unwrap(), []);
    }
}
