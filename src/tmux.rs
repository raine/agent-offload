use anyhow::{Context, Result, bail};
use std::env;
use std::path::Path;
use std::process::Command;

pub fn split_window(launcher_file: &Path, cwd: &Path) -> Result<String> {
    let pane_id = agent_pane()?;
    let target = split_target(&pane_id)?;

    let output = Command::new("tmux")
        .arg("split-window")
        .arg(target.direction.as_arg())
        .arg("-t")
        .arg(&target.pane_id)
        .arg("-c")
        .arg(cwd)
        .arg("-P")
        .arg("-F")
        .arg("#{pane_id}")
        .arg("sh")
        .arg(launcher_file)
        .output()
        .context("could not run tmux split-window")?;

    if !output.status.success() {
        return tmux_error("split-window", output.stderr);
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneStatus {
    Alive,
    Dead,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SplitTarget {
    pane_id: String,
    direction: SplitDirection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SplitDirection {
    Horizontal,
    Vertical,
}

impl SplitDirection {
    fn as_arg(self) -> &'static str {
        match self {
            Self::Horizontal => "-h",
            Self::Vertical => "-v",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PaneGeometry {
    pane_id: String,
    left: u16,
    top: u16,
    width: u16,
    height: u16,
}

pub fn pane_status(pane_id: &str) -> Result<PaneStatus> {
    let output = Command::new("tmux")
        .arg("display-message")
        .arg("-t")
        .arg(pane_id)
        .arg("-p")
        .arg("#{pane_dead}")
        .output()
        .context("could not check tmux pane")?;

    if !output.status.success() {
        return Ok(PaneStatus::Missing);
    }

    Ok(parse_pane_status(&output.stdout))
}

fn parse_pane_status(stdout: &[u8]) -> PaneStatus {
    if String::from_utf8_lossy(stdout).trim() == "1" {
        PaneStatus::Dead
    } else {
        PaneStatus::Alive
    }
}

fn split_target(agent_pane_id: &str) -> Result<SplitTarget> {
    let output = Command::new("tmux")
        .arg("list-panes")
        .arg("-F")
        .arg("#{pane_id}\t#{pane_left}\t#{pane_top}\t#{pane_width}\t#{pane_height}")
        .output()
        .context("could not list tmux panes")?;

    if !output.status.success() {
        return tmux_error("list-panes", output.stderr);
    }

    let panes = parse_pane_geometries(&output.stdout)?;
    Ok(choose_split_target(agent_pane_id, &panes))
}

fn choose_split_target(agent_pane_id: &str, panes: &[PaneGeometry]) -> SplitTarget {
    if let Some(agent_pane) = panes.iter().find(|pane| pane.pane_id == agent_pane_id)
        && let Some(right_pane) = panes
            .iter()
            .filter(|pane| pane.left > agent_pane.left)
            .max_by_key(|pane| (pane.left, u32::from(pane.width) * u32::from(pane.height)))
    {
        return SplitTarget {
            pane_id: right_pane.pane_id.clone(),
            direction: SplitDirection::Vertical,
        };
    }

    SplitTarget {
        pane_id: agent_pane_id.to_string(),
        direction: SplitDirection::Horizontal,
    }
}

fn parse_pane_geometries(stdout: &[u8]) -> Result<Vec<PaneGeometry>> {
    String::from_utf8_lossy(stdout)
        .lines()
        .map(parse_pane_geometry)
        .collect()
}

fn parse_pane_geometry(line: &str) -> Result<PaneGeometry> {
    let mut parts = line.split('\t');
    let pane_id = parts
        .next()
        .filter(|pane_id| !pane_id.is_empty())
        .context("tmux pane row is missing pane id")?;
    let left = parse_pane_dimension(parts.next(), "left")?;
    let top = parse_pane_dimension(parts.next(), "top")?;
    let width = parse_pane_dimension(parts.next(), "width")?;
    let height = parse_pane_dimension(parts.next(), "height")?;

    if parts.next().is_some() {
        bail!("tmux pane row has too many fields: {line}");
    }

    Ok(PaneGeometry {
        pane_id: pane_id.to_string(),
        left,
        top,
        width,
        height,
    })
}

fn parse_pane_dimension(value: Option<&str>, name: &str) -> Result<u16> {
    value
        .context("tmux pane row is missing dimension")?
        .parse()
        .with_context(|| format!("tmux pane row has invalid {name}"))
}

pub fn kill_pane(pane_id: &str) -> Result<()> {
    let output = Command::new("tmux")
        .arg("kill-pane")
        .arg("-t")
        .arg(pane_id)
        .output()
        .context("could not run tmux kill-pane")?;

    if !output.status.success() {
        return tmux_error("kill-pane", output.stderr);
    }

    Ok(())
}

fn agent_pane() -> Result<String> {
    let pane_id = env::var("TMUX_PANE").context("could not read agent tmux pane")?;
    let pane_id = pane_id.trim().to_string();
    if pane_id.is_empty() {
        bail!("TMUX_PANE is empty");
    }

    Ok(pane_id)
}

fn tmux_error<T>(command: &str, stderr: Vec<u8>) -> Result<T> {
    let stderr = String::from_utf8_lossy(&stderr);
    bail!("tmux {command} failed: {stderr}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_pane_status_alive() {
        assert_eq!(parse_pane_status(b"0\n"), PaneStatus::Alive);
    }

    #[test]
    fn test_parse_pane_status_dead() {
        assert_eq!(parse_pane_status(b"1\n"), PaneStatus::Dead);
    }

    #[test]
    fn test_choose_split_target_uses_right_pane_bottom_split() {
        let panes = vec![
            PaneGeometry {
                pane_id: "%2477".to_string(),
                left: 0,
                top: 0,
                width: 107,
                height: 24,
            },
            PaneGeometry {
                pane_id: "%2705".to_string(),
                left: 0,
                top: 25,
                width: 107,
                height: 24,
            },
            PaneGeometry {
                pane_id: "%2562".to_string(),
                left: 108,
                top: 0,
                width: 105,
                height: 49,
            },
        ];

        assert_eq!(
            choose_split_target("%2477", &panes),
            SplitTarget {
                pane_id: "%2562".to_string(),
                direction: SplitDirection::Vertical,
            }
        );
    }

    #[test]
    fn test_choose_split_target_falls_back_to_current_pane() {
        let panes = vec![PaneGeometry {
            pane_id: "%2477".to_string(),
            left: 0,
            top: 0,
            width: 107,
            height: 24,
        }];

        assert_eq!(
            choose_split_target("%2477", &panes),
            SplitTarget {
                pane_id: "%2477".to_string(),
                direction: SplitDirection::Horizontal,
            }
        );
    }

    #[test]
    fn test_parse_pane_geometries() {
        assert_eq!(
            parse_pane_geometries(b"%2477\t0\t0\t107\t24\n%2562\t108\t0\t105\t49\n").unwrap(),
            vec![
                PaneGeometry {
                    pane_id: "%2477".to_string(),
                    left: 0,
                    top: 0,
                    width: 107,
                    height: 24,
                },
                PaneGeometry {
                    pane_id: "%2562".to_string(),
                    left: 108,
                    top: 0,
                    width: 105,
                    height: 49,
                },
            ]
        );
    }
}
