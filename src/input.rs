use std::{
    collections::{BTreeMap, BTreeSet},
    fmt, fs,
    io::{self, Write},
    path::Path,
    str::FromStr,
};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use crate::config::is_flash_key;

pub const INPUT_SCHEMA_VERSION: u16 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum InputPreset {
    Authentic,
    Modern,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InputConfig {
    pub version: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<InputPreset>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub players: Vec<Player>,
}

impl Default for InputConfig {
    fn default() -> Self {
        Self {
            version: INPUT_SCHEMA_VERSION,
            preset: None,
            players: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Player {
    pub id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub devices: Vec<InputDevice>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeviceKind {
    Keyboard,
    Gamepad,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FieldUpdate<T> {
    Keep,
    Set(T),
    Clear,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InputDevice {
    pub id: String,
    pub kind: DeviceKind,
    #[serde(default = "enabled", skip_serializing_if = "is_enabled")]
    pub enabled: bool,
    /// Logical device selector. Ruffle 0.7.1 cannot enforce it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
    /// Normalized axis deadzone in [0, 1). Ruffle 0.7.1 cannot enforce it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadzone: Option<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bindings: Vec<Binding>,
}

const fn enabled() -> bool {
    true
}

const fn is_enabled(value: &bool) -> bool {
    *value
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub source: InputSource,
    pub target: String,
    /// Additional simultaneously held keyboard keys.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chord: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum InputSource {
    Key {
        key: String,
    },
    Button {
        button: String,
    },
    Axis {
        axis: String,
        direction: AxisDirection,
        #[serde(default = "default_axis_threshold")]
        threshold: f32,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AxisDirection {
    Negative,
    Positive,
}

const fn default_axis_threshold() -> f32 {
    0.5
}

#[derive(Debug, Eq, PartialEq)]
pub struct BackendReport {
    pub ruffle_gamepad_mappings: usize,
    pub limitations: Vec<String>,
}

impl InputConfig {
    pub fn read(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("cannot read input config {}", path.display()))?;
        let config = Self::from_toml(&text)
            .with_context(|| format!("invalid input config {}", path.display()))?;
        Ok(config)
    }

    pub fn from_toml(text: &str) -> Result<Self> {
        let config: Self = toml::from_str(text).context("invalid TOML input schema")?;
        config.validate()?;
        Ok(config)
    }

    pub fn from_json(text: &str) -> Result<Self> {
        let config: Self = serde_json::from_str(text).context("invalid JSON input schema")?;
        config.validate()?;
        Ok(config)
    }

    pub fn to_toml(&self) -> Result<String> {
        self.validate()?;
        toml::to_string_pretty(self).context("cannot serialize input config as TOML")
    }

    pub fn to_json(&self) -> Result<String> {
        self.validate()?;
        serde_json::to_string_pretty(self)
            .map(|text| format!("{text}\n"))
            .context("cannot serialize input config as JSON")
    }

    pub fn canonical_toml(&self) -> Result<String> {
        self.validate()?;
        let mut canonical = self.clone();
        canonical.preset = None;
        for player in &mut canonical.players {
            for device in &mut player.devices {
                for binding in &mut device.bindings {
                    binding.target.make_ascii_lowercase();
                    for key in &mut binding.chord {
                        key.make_ascii_lowercase();
                    }
                    binding.chord.sort();
                    match &mut binding.source {
                        InputSource::Key { key } => key.make_ascii_lowercase(),
                        InputSource::Button { button } => button.make_ascii_lowercase(),
                        InputSource::Axis { axis, .. } => axis.make_ascii_lowercase(),
                    }
                }
                device
                    .bindings
                    .sort_by_key(|binding| (binding.signature(), binding.target.clone()));
            }
            player.devices.sort_by(|left, right| left.id.cmp(&right.id));
        }
        canonical
            .players
            .sort_by(|left, right| left.id.cmp(&right.id));
        toml::to_string_pretty(&canonical).context("cannot canonicalize input config as TOML")
    }

    pub fn write_atomic(&self, path: &Path) -> Result<()> {
        atomic_write(path, self.to_toml()?.as_bytes())
    }

    pub fn write_new(&self, path: &Path) -> Result<()> {
        atomic_write_new(path, self.to_toml()?.as_bytes())
    }

    pub fn preset(preset: InputPreset) -> Self {
        match preset {
            InputPreset::Authentic => authentic_preset(),
            InputPreset::Modern => modern_preset(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == INPUT_SCHEMA_VERSION,
            "unsupported input schema version {}; expected {}",
            self.version,
            INPUT_SCHEMA_VERSION
        );
        if let Some(preset) = self.preset {
            ensure!(
                self == &Self::preset(preset),
                "input preset label '{preset:?}' does not match the built-in preset contents"
            );
        }
        let mut player_ids = BTreeSet::new();
        let mut global_gamepad_buttons = BTreeMap::<String, String>::new();
        for player in &self.players {
            validate_name("player", &player.id)?;
            ensure!(
                player_ids.insert(player.id.as_str()),
                "duplicate player id '{}'",
                player.id
            );
            let mut device_ids = BTreeSet::new();
            for device in &player.devices {
                validate_name("device", &device.id)?;
                ensure!(
                    device_ids.insert(device.id.as_str()),
                    "duplicate device id '{}' for player '{}'",
                    device.id,
                    player.id
                );
                if let Some(selector) = &device.selector {
                    validate_name("device selector", selector)?;
                }
                if let Some(deadzone) = device.deadzone {
                    ensure!(
                        device.kind == DeviceKind::Gamepad,
                        "deadzone is valid only for gamepad device '{}:{}'",
                        player.id,
                        device.id
                    );
                    ensure!(
                        deadzone.is_finite() && (0.0..1.0).contains(&deadzone),
                        "deadzone for '{}:{}' must be finite and in [0, 1)",
                        player.id,
                        device.id
                    );
                }
                let mut controls = BTreeMap::<String, String>::new();
                for binding in &device.bindings {
                    validate_binding(device, binding)?;
                    let signature = binding.signature();
                    if let Some(previous) =
                        controls.insert(signature.clone(), binding.target.clone())
                    {
                        bail!(
                            "conflicting binding for '{}:{}:{}': '{}' and '{}'",
                            player.id,
                            device.id,
                            signature,
                            previous,
                            binding.target
                        );
                    }
                    if device.enabled
                        && binding.chord.is_empty()
                        && let InputSource::Button { button } = &binding.source
                        && let Some(previous) = global_gamepad_buttons.insert(
                            button.to_ascii_lowercase(),
                            binding.target.to_ascii_lowercase(),
                        )
                        && !previous.eq_ignore_ascii_case(&binding.target)
                    {
                        bail!(
                            "Ruffle conflict for global gamepad button '{button}': targets '{previous}' and '{}' across players/devices",
                            binding.target
                        );
                    }
                }
            }
        }
        Ok(())
    }

    pub fn backend_report(&self) -> Result<BackendReport> {
        self.validate()?;
        let mut limitations = BTreeSet::new();
        let mut mappings = BTreeMap::new();
        let enabled_devices: Vec<_> = self
            .players
            .iter()
            .flat_map(|player| &player.devices)
            .filter(|device| device.enabled)
            .collect();
        if enabled_devices
            .iter()
            .any(|device| device.selector.is_some())
        {
            limitations.insert("Ruffle 0.7.1 cannot select a physical device by id".to_owned());
        }
        if enabled_devices
            .iter()
            .filter(|device| device.kind == DeviceKind::Gamepad)
            .count()
            > 1
        {
            limitations.insert(
                "Ruffle 0.7.1 applies gamepad mappings globally, not per player/device".to_owned(),
            );
        }
        for device in enabled_devices {
            if device.deadzone.is_some() {
                limitations.insert("Ruffle 0.7.1 exposes no deadzone argument".to_owned());
            }
            for binding in &device.bindings {
                if !binding.chord.is_empty() {
                    limitations.insert("Ruffle 0.7.1 exposes no chord/hotkey argument".to_owned());
                }
                match &binding.source {
                    InputSource::Button { button } if binding.chord.is_empty() => {
                        mappings.insert(
                            button.to_ascii_lowercase(),
                            binding.target.to_ascii_lowercase(),
                        );
                    }
                    InputSource::Button { .. } => {}
                    InputSource::Axis { .. } => {
                        limitations
                            .insert("Ruffle 0.7.1 exposes no axis mapping argument".to_owned());
                    }
                    InputSource::Key { key } if !key.eq_ignore_ascii_case(&binding.target) => {
                        limitations.insert(
                            "Ruffle 0.7.1 exposes no keyboard remapping argument".to_owned(),
                        );
                    }
                    InputSource::Key { .. } => {}
                }
            }
        }
        Ok(BackendReport {
            ruffle_gamepad_mappings: mappings.len(),
            limitations: limitations.into_iter().collect(),
        })
    }

    pub fn ruffle_mappings(&self) -> Result<BTreeMap<String, String>> {
        let report = self.backend_report()?;
        if !report.limitations.is_empty() {
            bail!(
                "input profile cannot be applied exactly by pinned Ruffle 0.7.1: {}; use `kof-wing input validate` for backend limits",
                report.limitations.join("; ")
            );
        }
        let mut mappings = BTreeMap::new();
        for device in self
            .players
            .iter()
            .flat_map(|player| &player.devices)
            .filter(|device| device.enabled)
        {
            for binding in &device.bindings {
                if let InputSource::Button { button } = &binding.source {
                    mappings.insert(
                        button.to_ascii_lowercase(),
                        binding.target.to_ascii_lowercase(),
                    );
                }
            }
        }
        Ok(mappings)
    }

    pub fn enabled_keyboard_devices(&self) -> usize {
        self.enabled_devices(DeviceKind::Keyboard)
    }

    pub fn enabled_gamepad_devices(&self) -> usize {
        self.enabled_devices(DeviceKind::Gamepad)
    }

    fn enabled_devices(&self, kind: DeviceKind) -> usize {
        self.players
            .iter()
            .flat_map(|player| &player.devices)
            .filter(|device| device.enabled && device.kind == kind)
            .count()
    }

    pub fn set_binding(
        &mut self,
        player_id: &str,
        device_id: &str,
        kind: DeviceKind,
        selector: FieldUpdate<String>,
        deadzone: FieldUpdate<f32>,
        binding: Binding,
    ) -> Result<()> {
        validate_name("player", player_id)?;
        validate_name("device", device_id)?;
        self.preset = None;
        let player = if let Some(index) = self.players.iter().position(|item| item.id == player_id)
        {
            &mut self.players[index]
        } else {
            self.players.push(Player {
                id: player_id.to_owned(),
                devices: Vec::new(),
            });
            self.players.last_mut().expect("player was just inserted")
        };
        let device =
            if let Some(index) = player.devices.iter().position(|item| item.id == device_id) {
                &mut player.devices[index]
            } else {
                player.devices.push(InputDevice {
                    id: device_id.to_owned(),
                    kind,
                    enabled: true,
                    selector: None,
                    deadzone: None,
                    bindings: Vec::new(),
                });
                player.devices.last_mut().expect("device was just inserted")
            };
        ensure!(
            device.kind == kind,
            "device '{}:{}' already exists as {:?}",
            player_id,
            device_id,
            device.kind
        );
        match selector {
            FieldUpdate::Keep => {}
            FieldUpdate::Set(value) => device.selector = Some(value),
            FieldUpdate::Clear => device.selector = None,
        }
        match deadzone {
            FieldUpdate::Keep => {}
            FieldUpdate::Set(value) => device.deadzone = Some(value),
            FieldUpdate::Clear => device.deadzone = None,
        }
        let signature = binding.signature();
        if let Some(existing) = device
            .bindings
            .iter_mut()
            .find(|item| item.signature() == signature)
        {
            *existing = binding;
        } else {
            device.bindings.push(binding);
        }
        self.validate()
    }
}

impl Binding {
    fn signature(&self) -> String {
        let mut chord: Vec<_> = self
            .chord
            .iter()
            .map(|key| key.to_ascii_lowercase())
            .collect();
        chord.sort();
        format!("{}+{}", self.source, chord.join("+"))
    }
}

impl fmt::Display for InputSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Key { key } => write!(formatter, "key:{}", key.to_ascii_lowercase()),
            Self::Button { button } => write!(formatter, "button:{}", button.to_ascii_lowercase()),
            Self::Axis {
                axis, direction, ..
            } => write!(
                formatter,
                "axis:{}:{}",
                axis.to_ascii_lowercase(),
                match direction {
                    AxisDirection::Negative => "negative",
                    AxisDirection::Positive => "positive",
                }
            ),
        }
    }
}

impl FromStr for InputSource {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        let mut parts = value.split(':');
        let kind = parts.next().unwrap_or_default();
        let name = parts.next().unwrap_or_default();
        ensure!(
            !name.is_empty(),
            "input source must be key:<key>, button:<button>, or axis:<axis>:<positive|negative>"
        );
        let source = match kind {
            "key" if parts.next().is_none() => Self::Key {
                key: name.to_owned(),
            },
            "button" if parts.next().is_none() => Self::Button {
                button: name.to_owned(),
            },
            "axis" => {
                let direction = match parts.next() {
                    Some("positive" | "+") => AxisDirection::Positive,
                    Some("negative" | "-") => AxisDirection::Negative,
                    _ => bail!("axis source direction must be positive or negative"),
                };
                ensure!(
                    parts.next().is_none(),
                    "input source has too many components"
                );
                Self::Axis {
                    axis: name.to_owned(),
                    direction,
                    threshold: default_axis_threshold(),
                }
            }
            _ => bail!(
                "input source must be key:<key>, button:<button>, or axis:<axis>:<positive|negative>"
            ),
        };
        Ok(source)
    }
}

fn validate_binding(device: &InputDevice, binding: &Binding) -> Result<()> {
    ensure!(
        is_flash_key(&binding.target),
        "invalid Flash target key '{}'",
        binding.target
    );
    let mut chord = BTreeSet::new();
    for key in &binding.chord {
        validate_control_name("chord key", key)?;
        ensure!(
            chord.insert(key.to_ascii_lowercase()),
            "duplicate chord key '{key}'"
        );
    }
    match (&device.kind, &binding.source) {
        (DeviceKind::Keyboard, InputSource::Key { key }) => validate_control_name("key", key)?,
        (DeviceKind::Gamepad, InputSource::Button { button }) => {
            ensure!(
                is_gamepad_button(button),
                "invalid Ruffle gamepad button '{button}'"
            );
        }
        (
            DeviceKind::Gamepad,
            InputSource::Axis {
                axis, threshold, ..
            },
        ) => {
            validate_control_name("axis", axis)?;
            ensure!(
                threshold.is_finite() && *threshold > 0.0 && *threshold <= 1.0,
                "axis threshold must be finite and in (0, 1]"
            );
            if let Some(deadzone) = device.deadzone {
                ensure!(
                    *threshold > deadzone,
                    "axis threshold must be greater than device deadzone"
                );
            }
        }
        (DeviceKind::Keyboard, _) => {
            bail!("keyboard device '{}' accepts only key sources", device.id)
        }
        (DeviceKind::Gamepad, InputSource::Key { .. }) => {
            bail!(
                "gamepad device '{}' accepts only button or axis sources",
                device.id
            )
        }
    }
    Ok(())
}

pub fn validate_name(kind: &str, value: &str) -> Result<()> {
    let valid = !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
    ensure!(
        valid,
        "{kind} names must be 1-64 ASCII letters, digits, '-' or '_'"
    );
    Ok(())
}

fn validate_control_name(kind: &str, value: &str) -> Result<()> {
    let valid = !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
    ensure!(
        valid,
        "{kind} must be a safe 1-64 character ASCII identifier"
    );
    Ok(())
}

pub fn is_gamepad_button(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "north"
            | "east"
            | "south"
            | "west"
            | "d-pad-up"
            | "d-pad-down"
            | "d-pad-left"
            | "d-pad-right"
            | "left-trigger"
            | "left-trigger2"
            | "right-trigger"
            | "right-trigger2"
            | "select"
            | "start"
    )
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let temporary = write_temporary(path, bytes)?;
    temporary
        .persist(path)
        .map_err(|error| io::Error::new(error.error.kind(), error.error))
        .with_context(|| format!("cannot atomically replace {}", path.display()))?;
    Ok(())
}

pub fn atomic_write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let temporary = write_temporary(path, bytes)?;
    temporary
        .persist_noclobber(path)
        .map_err(|error| io::Error::new(error.error.kind(), error.error))
        .with_context(|| format!("cannot create file without overwriting {}", path.display()))?;
    Ok(())
}

fn write_temporary(path: &Path, bytes: &[u8]) -> Result<NamedTempFile> {
    let parent = destination_parent(path);
    fs::create_dir_all(parent).with_context(|| format!("cannot create {}", parent.display()))?;
    let mut temporary = NamedTempFile::new_in(parent)
        .with_context(|| format!("cannot create temporary file in {}", parent.display()))?;
    temporary
        .write_all(bytes)
        .and_then(|()| temporary.flush())
        .and_then(|()| temporary.as_file().sync_all())
        .with_context(|| format!("cannot write temporary file for {}", path.display()))?;
    Ok(temporary)
}

fn destination_parent(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn keyboard_device(id: &str, keys: &[(&str, &str)]) -> InputDevice {
    InputDevice {
        id: id.to_owned(),
        kind: DeviceKind::Keyboard,
        enabled: true,
        selector: None,
        deadzone: None,
        bindings: keys
            .iter()
            .map(|(key, target)| Binding {
                source: InputSource::Key {
                    key: (*key).to_owned(),
                },
                target: (*target).to_owned(),
                chord: Vec::new(),
            })
            .collect(),
    }
}

fn authentic_preset() -> InputConfig {
    InputConfig {
        version: INPUT_SCHEMA_VERSION,
        preset: Some(InputPreset::Authentic),
        players: vec![
            Player {
                id: "p1".to_owned(),
                devices: vec![keyboard_device(
                    "keyboard-p1",
                    &[
                        ("w", "w"),
                        ("a", "a"),
                        ("s", "s"),
                        ("d", "d"),
                        ("j", "j"),
                        ("k", "k"),
                        ("l", "l"),
                        ("u", "u"),
                        ("i", "i"),
                        ("o", "o"),
                    ],
                )],
            },
            Player {
                id: "p2".to_owned(),
                devices: vec![keyboard_device(
                    "keyboard-p2",
                    &[
                        ("up", "up"),
                        ("down", "down"),
                        ("left", "left"),
                        ("right", "right"),
                        ("key1", "key1"),
                        ("key2", "key2"),
                        ("key3", "key3"),
                        ("key4", "key4"),
                        ("key5", "key5"),
                        ("key6", "key6"),
                    ],
                )],
            },
        ],
    }
}

fn modern_preset() -> InputConfig {
    InputConfig {
        version: INPUT_SCHEMA_VERSION,
        preset: Some(InputPreset::Modern),
        players: vec![Player {
            id: "p1".to_owned(),
            devices: vec![InputDevice {
                id: "gamepad-p1".to_owned(),
                kind: DeviceKind::Gamepad,
                enabled: true,
                selector: None,
                deadzone: None,
                bindings: [
                    ("d-pad-up", "w"),
                    ("d-pad-down", "s"),
                    ("d-pad-left", "a"),
                    ("d-pad-right", "d"),
                    ("west", "j"),
                    ("south", "k"),
                    ("east", "l"),
                    ("north", "u"),
                    ("left-trigger", "i"),
                    ("right-trigger", "o"),
                ]
                .into_iter()
                .map(|(button, target)| Binding {
                    source: InputSource::Button {
                        button: button.to_owned(),
                    },
                    target: target.to_owned(),
                    chord: Vec::new(),
                })
                .collect(),
            }],
        }],
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn empty_and_presets_are_versioned_valid_and_round_trip() {
        for config in [
            InputConfig::default(),
            InputConfig::preset(InputPreset::Authentic),
            InputConfig::preset(InputPreset::Modern),
        ] {
            let toml = config.to_toml().expect("serialize TOML");
            assert_eq!(
                InputConfig::from_toml(&toml).unwrap().version,
                INPUT_SCHEMA_VERSION
            );
            let json = config.to_json().expect("serialize JSON");
            assert_eq!(
                InputConfig::from_json(&json).unwrap().version,
                INPUT_SCHEMA_VERSION
            );
        }
    }

    #[test]
    fn canonical_form_ignores_semantically_irrelevant_order_and_case() {
        let preset = InputConfig::preset(InputPreset::Modern);
        let mut first = preset.clone();
        first.preset = None;
        let mut reordered = first.clone();
        reordered.players[0].devices[0].bindings.reverse();
        reordered.players[0].devices[0].bindings[0]
            .target
            .make_ascii_uppercase();
        let InputSource::Button { button } =
            &mut reordered.players[0].devices[0].bindings[0].source
        else {
            panic!("modern preset contains only buttons");
        };
        button.make_ascii_uppercase();
        assert_ne!(first.to_toml().unwrap(), reordered.to_toml().unwrap());
        assert_eq!(
            first.canonical_toml().unwrap(),
            reordered.canonical_toml().unwrap()
        );
        assert_eq!(
            preset.canonical_toml().unwrap(),
            first.canonical_toml().unwrap()
        );
    }

    #[test]
    fn rejects_false_preset_labels_and_nonsensical_ranges() {
        let mut mislabeled = InputConfig::preset(InputPreset::Modern);
        mislabeled.players[0].devices[0].bindings.pop();
        assert!(
            mislabeled
                .validate()
                .unwrap_err()
                .to_string()
                .contains("label")
        );

        let mut keyboard = InputConfig::default();
        assert!(
            keyboard
                .set_binding(
                    "p1",
                    "keyboard",
                    DeviceKind::Keyboard,
                    FieldUpdate::Keep,
                    FieldUpdate::Set(0.1),
                    Binding {
                        source: "key:w".parse().unwrap(),
                        target: "w".into(),
                        chord: vec![],
                    },
                )
                .unwrap_err()
                .to_string()
                .contains("gamepad")
        );

        let mut axis = InputConfig::default();
        assert!(
            axis.set_binding(
                "p1",
                "pad",
                DeviceKind::Gamepad,
                FieldUpdate::Keep,
                FieldUpdate::Keep,
                Binding {
                    source: InputSource::Axis {
                        axis: "left-x".into(),
                        direction: AxisDirection::Positive,
                        threshold: 0.0,
                    },
                    target: "right".into(),
                    chord: vec![],
                },
            )
            .is_err()
        );
    }

    #[test]
    fn binding_updates_distinguish_keep_set_and_clear() {
        let binding = Binding {
            source: "button:south".parse().unwrap(),
            target: "k".into(),
            chord: vec![],
        };
        let mut config = InputConfig::default();
        config
            .set_binding(
                "p1",
                "pad",
                DeviceKind::Gamepad,
                FieldUpdate::Set("primary".into()),
                FieldUpdate::Set(0.2),
                binding.clone(),
            )
            .unwrap();
        config
            .set_binding(
                "p1",
                "pad",
                DeviceKind::Gamepad,
                FieldUpdate::Keep,
                FieldUpdate::Keep,
                binding.clone(),
            )
            .unwrap();
        assert_eq!(
            config.players[0].devices[0].selector.as_deref(),
            Some("primary")
        );
        assert_eq!(config.players[0].devices[0].deadzone, Some(0.2));

        config
            .set_binding(
                "p1",
                "pad",
                DeviceKind::Gamepad,
                FieldUpdate::Clear,
                FieldUpdate::Clear,
                binding,
            )
            .unwrap();
        assert_eq!(config.players[0].devices[0].selector, None);
        assert_eq!(config.players[0].devices[0].deadzone, None);
    }

    #[test]
    fn detects_schema_name_binding_axis_and_global_backend_conflicts() {
        let invalid_version = InputConfig {
            version: 2,
            ..InputConfig::default()
        };
        assert!(
            invalid_version
                .validate()
                .unwrap_err()
                .to_string()
                .contains("version")
        );

        let mut config = InputConfig::default();
        config
            .set_binding(
                "p1",
                "pad",
                DeviceKind::Gamepad,
                FieldUpdate::Keep,
                FieldUpdate::Keep,
                Binding {
                    source: "button:south".parse().unwrap(),
                    target: "j".into(),
                    chord: vec![],
                },
            )
            .unwrap();
        let error = config
            .set_binding(
                "p2",
                "pad",
                DeviceKind::Gamepad,
                FieldUpdate::Keep,
                FieldUpdate::Keep,
                Binding {
                    source: "button:south".parse().unwrap(),
                    target: "k".into(),
                    chord: vec![],
                },
            )
            .unwrap_err();
        assert!(error.to_string().contains("Ruffle conflict"));

        let mut axis = InputConfig::default();
        assert!(
            axis.set_binding(
                "p1",
                "pad",
                DeviceKind::Gamepad,
                FieldUpdate::Keep,
                FieldUpdate::Set(0.8),
                Binding {
                    source: InputSource::Axis {
                        axis: "left-x".into(),
                        direction: AxisDirection::Positive,
                        threshold: 0.5
                    },
                    target: "right".into(),
                    chord: vec![]
                },
            )
            .is_err()
        );
    }

    #[test]
    fn report_and_compile_match_real_ruffle_capabilities() {
        let modern = InputConfig::preset(InputPreset::Modern);
        assert_eq!(modern.backend_report().unwrap().ruffle_gamepad_mappings, 10);
        assert_eq!(modern.ruffle_mappings().unwrap()["south"], "k");

        let authentic = InputConfig::preset(InputPreset::Authentic);
        assert!(authentic.ruffle_mappings().unwrap().is_empty());
    }

    #[test]
    fn atomic_write_replaces_and_never_leaves_a_partial_document() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("input.toml");
        InputConfig::default().write_atomic(&path).unwrap();
        InputConfig::preset(InputPreset::Modern)
            .write_atomic(&path)
            .unwrap();
        assert_eq!(
            InputConfig::read(&path).unwrap().preset,
            Some(InputPreset::Modern)
        );
        assert_eq!(destination_parent(Path::new("input.toml")), Path::new("."));
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_replaces_a_symlink_without_touching_its_target() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().unwrap();
        let target = temp.path().join("outside.toml");
        let path = temp.path().join("input.toml");
        fs::write(&target, b"outside").unwrap();
        symlink(&target, &path).unwrap();

        InputConfig::default().write_atomic(&path).unwrap();

        assert_eq!(fs::read(&target).unwrap(), b"outside");
        assert!(
            !fs::symlink_metadata(&path)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        InputConfig::read(&path).unwrap();
    }

    #[test]
    fn parses_all_source_forms_and_rejects_unsafe_components() {
        assert!(matches!(
            "key:w".parse::<InputSource>().unwrap(),
            InputSource::Key { .. }
        ));
        assert!(matches!(
            "button:south".parse::<InputSource>().unwrap(),
            InputSource::Button { .. }
        ));
        assert!(matches!(
            "axis:left-x:-".parse::<InputSource>().unwrap(),
            InputSource::Axis {
                direction: AxisDirection::Negative,
                ..
            }
        ));
        assert!(validate_name("player", "../escape").is_err());
    }
}
