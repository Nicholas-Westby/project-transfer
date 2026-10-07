//! This computer's own settings: name, Projects folder, theme, ignore list.

use super::{Core, lock};
use crate::ignore_rules::{DEFAULT_IGNORES, IgnoreSpec, Matcher, trim};
use crate::model::{InstanceSettings, ThemeChoice};
use anyhow::{Context, bail};
use std::path::PathBuf;

impl Core {
    /// Saves before memory changes, so memory never claims what the disk lacks.
    pub(super) async fn update_settings(
        &self,
        f: impl FnOnce(&mut InstanceSettings),
    ) -> anyhow::Result<InstanceSettings> {
        let mut guard = self.shared.settings.write().await;
        let mut next = guard.clone();
        f(&mut next);
        self.shared
            .store
            .save_instance(&next)
            .context("Could not save the settings")?;
        *guard = next.clone();
        let me = next.clone();
        self.ui.update(|s| s.me = me);
        Ok(next)
    }

    pub(super) async fn rename(&self, name: String) -> anyhow::Result<()> {
        let name = name.trim().to_string();
        if name.is_empty() {
            bail!("Enter a name for this computer.");
        }
        let n = name.clone();
        self.update_settings(|s| s.name = n).await?;
        if let Some(d) = lock(&self.discovery).as_ref()
            && let Err(e) = d.rename(&name)
        {
            self.ui.warn(format!(
                "Renamed, but could not announce the new name on the network ({e:#}). Paired \
                 computers see it after the next restart."
            ));
        }
        self.ui.info(format!("This computer is now called {name}."));
        Ok(())
    }

    pub(super) async fn set_projects_folder(&self, path: PathBuf) -> anyhow::Result<()> {
        let shown = path.display().to_string();
        self.update_settings(|s| s.projects_folder = path).await?;
        self.ui.info(format!(
            "Projects arriving from other computers now go to {shown}."
        ));
        Ok(())
    }

    pub(super) async fn set_theme(&self, theme: ThemeChoice) -> anyhow::Result<()> {
        self.update_settings(|s| s.theme = theme).await?;
        self.ui.info(match theme {
            ThemeChoice::Dark => "Switched to the dark theme.",
            ThemeChoice::Light => "Switched to the light theme.",
            ThemeChoice::System => "The theme now follows the system setting.",
        });
        Ok(())
    }

    pub(super) async fn set_ignores(
        &self,
        extra: Vec<String>,
        always_include: Vec<String>,
        removed_defaults: Vec<String>,
    ) -> anyhow::Result<()> {
        self.save_ignores(extra, always_include, removed_defaults)
            .await?;
        self.ui
            .info("Updated the ignore list. It applies from the next transfer.");
        Ok(())
    }

    pub(super) async fn add_ignore(&self, pattern: String) -> anyhow::Result<()> {
        let pattern = trim(&pattern).to_string();
        if pattern.is_empty() {
            bail!("Enter a pattern to ignore.");
        }
        let s = self.shared.settings.read().await.clone();
        let (mut extra, mut removed) = (s.extra_ignores, s.removed_default_ignores);
        let done = if removed.contains(&pattern) {
            removed.retain(|r| *r != pattern);
            format!("Turned {pattern} back on in the ignore list.")
        } else if extra.contains(&pattern) || DEFAULT_IGNORES.contains(&pattern.as_str()) {
            self.ui
                .info(format!("{pattern} is already on the ignore list."));
            return Ok(());
        } else {
            extra.push(pattern.clone());
            format!("Added {pattern} to the ignore list.")
        };
        self.save_ignores(extra, s.always_include, removed).await?;
        self.ui.info(done);
        self.compare_again()
    }

    /// Checks the whole list before saving any of it, so a bad pattern
    /// changes nothing.
    async fn save_ignores(
        &self,
        extra: Vec<String>,
        always_include: Vec<String>,
        removed_defaults: Vec<String>,
    ) -> anyhow::Result<()> {
        let clean = |v: Vec<String>| -> Vec<String> {
            v.into_iter()
                .map(|p| trim(&p).to_string())
                .filter(|p| !p.is_empty())
                .collect()
        };
        let mut candidate = self.shared.settings.read().await.clone();
        candidate.extra_ignores = clean(extra);
        candidate.always_include = clean(always_include);
        candidate.removed_default_ignores = clean(removed_defaults);
        if let Err(e) = Matcher::new(&IgnoreSpec::from_settings(&candidate)) {
            bail!("The ignore list was not changed: {e:#}. Fix the pattern and try again.");
        }
        self.update_settings(|s| {
            s.extra_ignores = candidate.extra_ignores;
            s.always_include = candidate.always_include;
            s.removed_default_ignores = candidate.removed_default_ignores;
        })
        .await?;
        Ok(())
    }

    pub(super) fn open_log_folder(&self) -> anyhow::Result<()> {
        let dir = self.shared.store.logs_dir();
        let opener = if cfg!(windows) { "explorer" } else { "open" };
        let mut child = std::process::Command::new(opener)
            .arg(&dir)
            .spawn()
            .with_context(|| format!("Could not open the log folder {}", dir.display()))?;
        // Reaped off the runtime so the opener never lingers as a zombie.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }
}
