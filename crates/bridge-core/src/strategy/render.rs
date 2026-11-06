//! Rendering a strategy into the exact winws argument vector.

use super::{ArgValue, Strategy};
use crate::ports::{GameFilterPorts, PortSpec};

/// Resolves resource names to absolute paths. Implemented by the helper
/// (protected install/ProgramData directories) and by the preview renderer.
pub trait ResourceResolver {
    fn fake_path(&self, name: &str) -> Option<String>;
    fn list_path(&self, name: &str) -> Option<String>;
}

/// Renders `%BIN%name` / `%LISTS%name` placeholders, for UI previews only.
pub struct PreviewResolver;

impl ResourceResolver for PreviewResolver {
    fn fake_path(&self, name: &str) -> Option<String> {
        Some(format!("%BIN%{name}"))
    }
    fn list_path(&self, name: &str) -> Option<String> {
        Some(format!("%LISTS%{name}"))
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RenderError {
    #[error("ресурс «{0}» не найден")]
    MissingResource(String),
    #[error("некорректный список портов в {0}: {1}")]
    BadPorts(String, String),
}

const PORT_OPTS: &[&str] = &["--wf-tcp", "--wf-udp", "--filter-tcp", "--filter-udp"];

/// Produces the argument list (without the executable). Disabled rules are
/// skipped and `--new` separators are inserted only between emitted rules.
/// The caller must run [`super::validate::validate`] first; this function only
/// maps already validated data.
pub fn render_args(
    s: &Strategy,
    gf: &GameFilterPorts,
    res: &dyn ResourceResolver,
) -> Result<Vec<String>, RenderError> {
    let mut out = Vec::new();
    let push =
        |opt: &str, v: &Option<ArgValue>, out: &mut Vec<String>| -> Result<(), RenderError> {
            match v {
                None => out.push(opt.to_string()),
                Some(ArgValue::Text(t)) => {
                    let t = if PORT_OPTS.contains(&opt) {
                        PortSpec::parse(t)
                            .map_err(|e| RenderError::BadPorts(opt.into(), e.to_string()))?
                            .render(gf)
                    } else {
                        t.clone()
                    };
                    out.push(format!("{opt}={t}"));
                }
                Some(ArgValue::Fake { fake }) => {
                    let p = res
                        .fake_path(fake)
                        .ok_or_else(|| RenderError::MissingResource(fake.clone()))?;
                    out.push(format!("{opt}={p}"));
                }
                Some(ArgValue::List { list }) => {
                    let p = res
                        .list_path(list)
                        .ok_or_else(|| RenderError::MissingResource(list.clone()))?;
                    out.push(format!("{opt}={p}"));
                }
            }
            Ok(())
        };
    for a in &s.global {
        push(&a.opt, &a.value, &mut out)?;
    }
    let mut first = true;
    for rule in s.rules.iter().filter(|r| r.enabled) {
        if !first {
            out.push("--new".into());
        }
        first = false;
        for a in &rule.args {
            push(&a.opt, &a.value, &mut out)?;
        }
    }
    Ok(out)
}

/// One line per rule, for previews and textual diffs.
pub fn render_lines(s: &Strategy, gf: &GameFilterPorts) -> Vec<String> {
    let mut lines = Vec::new();
    let fmt = |a: &super::Arg| match &a.value {
        None => a.opt.clone(),
        Some(ArgValue::Text(t)) => {
            if PORT_OPTS.contains(&a.opt.as_str()) {
                match PortSpec::parse(t) {
                    Ok(p) => format!("{}={}", a.opt, p.render(gf)),
                    Err(_) => format!("{}={t}", a.opt),
                }
            } else {
                format!("{}={t}", a.opt)
            }
        }
        Some(ArgValue::Fake { fake }) => format!("{}=%BIN%{fake}", a.opt),
        Some(ArgValue::List { list }) => format!("{}=%LISTS%{list}", a.opt),
    };
    lines.push(s.global.iter().map(fmt).collect::<Vec<_>>().join(" "));
    for (i, r) in s.rules.iter().enumerate() {
        let body = r.args.iter().map(fmt).collect::<Vec<_>>().join(" ");
        let prefix = if r.enabled {
            ""
        } else {
            "# (выключено) "
        };
        lines.push(format!("{prefix}[{}] {body}", i + 1));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::GameFilter;
    use crate::strategy::{Arg, Rule, Strategy};

    #[test]
    fn renders_in_order_and_skips_disabled() {
        let mut s = Strategy::new_empty("t", "T");
        s.global = vec![Arg::text("--wf-tcp", "443,%GameFilterTCP%")];
        s.rules.push(Rule {
            label: None,
            enabled: false,
            args: vec![Arg::text("--filter-tcp", "80")],
        });
        s.rules.push(Rule {
            label: None,
            enabled: true,
            args: vec![
                Arg::text("--filter-tcp", "443"),
                Arg::fake("--dpi-desync-fake-tls", "a.bin"),
            ],
        });
        let args = render_args(&s, &GameFilter::default().ports(), &PreviewResolver).unwrap();
        assert_eq!(
            args,
            vec![
                "--wf-tcp=443,12",
                "--filter-tcp=443",
                "--hostlist=%LISTS%list-general.txt",
                "--dpi-desync=multisplit",
                "--dpi-desync-split-pos=1",
                "--new",
                "--filter-tcp=443",
                "--dpi-desync-fake-tls=%BIN%a.bin",
            ]
        );
    }
}
