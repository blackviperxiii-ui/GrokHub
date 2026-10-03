//! Absolute uinput pointer. Opening a device calls `UI_DEV_CREATE`.
//! Tests never construct [`UinputPointer`]; they use the descriptor in core.

use evdev::uinput::VirtualDevice;
use evdev::{AbsInfo, AbsoluteAxisCode, AttributeSet, BusType, InputEvent, InputId, KeyCode, PropType, RelativeAxisCode, UinputAbsSetup};
use grokhub_core::desktop_mcp::{uinput_abs_descriptor, uinput_access_denied_message, MonitorGeom, UinputAbsDescriptor};

const EV_SYN: u16 = 0;
const EV_KEY: u16 = 1;
const EV_REL: u16 = 2;
const EV_ABS: u16 = 3;
const SYN_REPORT: u16 = 0;
const REL_HWHEEL: u16 = 6;
const REL_WHEEL: u16 = 8;
const ABS_X: u16 = 0;
const ABS_Y: u16 = 1;

/// Codes this device can emit. Buttons, wheel keys, and the evdev map in `keys`.
const KEY_CODES: &[u16] = &[
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 28, 29, 30,
    31, 32, 33, 34, 35, 36, 37, 38, 42, 44, 45, 46, 47, 48, 49, 50, 56, 57, 59, 60, 61, 62, 63, 64,
    65, 66, 67, 68, 87, 88, 102, 103, 104, 105, 106, 107, 108, 109, 110, 111, 125, 183, 184, 185,
    186, 187, 188, 189, 190, 191, 192, 193, 194, 0x110, 0x111, 0x112, 0x140, 0x14a,
];

pub(crate) struct UinputPointer {
    dev: VirtualDevice,
    desc: UinputAbsDescriptor,
}

impl UinputPointer {
    pub(crate) fn open(monitors: &[MonitorGeom]) -> Result<Self, String> {
        ensure_access()?;
        let desc = uinput_abs_descriptor(monitors)?;
        let dev = build(&desc)?;
        Ok(Self { dev, desc })
    }

    pub(crate) fn move_abs(&mut self, x: i32, y: i32) -> Result<(), String> {
        let (ax, ay) = grokhub_core::desktop_mcp::map_point_to_uinput(&self.desc, x, y);
        self.emit(&[
            InputEvent::new(EV_ABS, ABS_X, ax),
            InputEvent::new(EV_ABS, ABS_Y, ay),
            InputEvent::new(EV_SYN, SYN_REPORT, 0),
        ])
    }

    pub(crate) fn button(&mut self, code: u16, down: bool) -> Result<(), String> {
        let value = if down { 1 } else { 0 };
        self.emit(&[
            InputEvent::new(EV_KEY, code, value),
            InputEvent::new(EV_SYN, SYN_REPORT, 0),
        ])
    }

    pub(crate) fn scroll(&mut self, dx: i32, dy: i32) -> Result<(), String> {
        let mut events = Vec::new();
        if dy != 0 {
            events.push(InputEvent::new(EV_REL, REL_WHEEL, dy));
        }
        if dx != 0 {
            events.push(InputEvent::new(EV_REL, REL_HWHEEL, dx));
        }
        if events.is_empty() {
            return Ok(());
        }
        events.push(InputEvent::new(EV_SYN, SYN_REPORT, 0));
        self.emit(&events)
    }

    pub(crate) fn key(&mut self, code: u16, down: bool) -> Result<(), String> {
        self.button(code, down)
    }

    fn emit(&mut self, events: &[InputEvent]) -> Result<(), String> {
        self.dev
            .emit(events)
            .map_err(|err| format!("absolute uinput: {err}"))
    }
}

fn ensure_access() -> Result<(), String> {
    let path = std::path::Path::new("/dev/uinput");
    if std::fs::OpenOptions::new().write(true).open(path).is_err() {
        return Err(uinput_access_denied_message().to_string());
    }
    Ok(())
}

fn build(desc: &UinputAbsDescriptor) -> Result<VirtualDevice, String> {
    let mut keys = AttributeSet::<KeyCode>::new();
    for code in KEY_CODES {
        keys.insert(KeyCode(*code));
    }
    let mut rel = AttributeSet::<RelativeAxisCode>::new();
    rel.insert(RelativeAxisCode::REL_WHEEL);
    rel.insert(RelativeAxisCode::REL_HWHEEL);
    let mut props = AttributeSet::<PropType>::new();
    props.insert(PropType::DIRECT);
    let abs_x = UinputAbsSetup::new(
        AbsoluteAxisCode::ABS_X,
        AbsInfo::new(0, desc.abs_x.minimum, desc.abs_x.maximum, 0, 0, 1),
    );
    let abs_y = UinputAbsSetup::new(
        AbsoluteAxisCode::ABS_Y,
        AbsInfo::new(0, desc.abs_y.minimum, desc.abs_y.maximum, 0, 0, 1),
    );
    VirtualDevice::builder()
        .map_err(|err| format!("absolute uinput: {err}"))?
        .name(desc.name)
        .input_id(InputId::new(BusType::BUS_USB, 0x1209, 0x2b2b, 1))
        .with_properties(&props)
        .map_err(|err| format!("absolute uinput: {err}"))?
        .with_keys(&keys)
        .map_err(|err| format!("absolute uinput: {err}"))?
        .with_absolute_axis(&abs_x)
        .map_err(|err| format!("absolute uinput: {err}"))?
        .with_absolute_axis(&abs_y)
        .map_err(|err| format!("absolute uinput: {err}"))?
        .with_relative_axes(&rel)
        .map_err(|err| format!("absolute uinput: {err}"))?
        .build()
        .map_err(|err| format!("absolute uinput: {err}"))
}
