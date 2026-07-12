use evdev::{
    AbsInfo, AbsoluteAxisCode, AttributeSet, EventType, InputEvent, KeyCode, PropType,
    RelativeAxisCode, UinputAbsSetup, uinput::VirtualDevice,
};
use std::{
    io::{self, Read, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    thread,
    time::Duration,
};

const READY: [u8; 4] = *b"BNUI";
const VERSION: u8 = 1;
const COMMAND_BYTES: usize = 9;
const AXIS_MAX: i32 = 65_535;
const DEFAULT_POINTER_SPACE: (i32, i32) = (2_400, 1_200);
const DEVICE_SETTLE: Duration = Duration::from_millis(500);
const CLICK_HOLD: Duration = Duration::from_millis(32);
const UINPUT_NAME_MAX_BYTES: usize = 79;

const MOVE_ABSOLUTE: u8 = 1;
const MOVE_RELATIVE: u8 = 2;
const BUTTON: u8 = 3;
const WHEEL: u8 = 4;
const KEY: u8 = 5;
const SHUTDOWN: u8 = 6;

pub struct NativeInput {
    child: Child,
    input: Option<ChildStdin>,
    output: ChildStdout,
    pointer_space: (i32, i32),
}

impl NativeInput {
    pub fn start(executable: &std::path::Path, seat_name: &str) -> Result<Self, String> {
        validate_seat_name(seat_name)?;
        let mut child = Command::new(executable)
            .args(["--role", "native-input", "--seat", seat_name])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| format!("start kernel virtual input process: {error}"))?;
        let input = child
            .stdin
            .take()
            .ok_or("kernel virtual input process has no command pipe")?;
        let mut output = child
            .stdout
            .take()
            .ok_or("kernel virtual input process has no acknowledgement pipe")?;
        let mut ready = [0_u8; 5];
        output
            .read_exact(&mut ready)
            .map_err(|error| format!("kernel virtual input did not become ready: {error}"))?;
        if ready[..4] != READY || ready[4] != VERSION {
            return Err("kernel virtual input returned an invalid handshake".to_owned());
        }
        Ok(Self {
            child,
            input: Some(input),
            output,
            pointer_space: DEFAULT_POINTER_SPACE,
        })
    }

    pub fn set_pointer_space(&mut self, width: i32, height: i32) -> Result<(), String> {
        if width <= 0 || height <= 0 {
            return Err(format!("invalid pointer space {width}x{height}"));
        }
        self.pointer_space = (width, height);
        Ok(())
    }

    pub fn prepare_pointer(&mut self) -> Result<(), String> {
        for _ in 0..96 {
            self.command(MOVE_RELATIVE, -24, -24)?;
        }
        Ok(())
    }

    pub fn move_pointer(&mut self, point: (i32, i32)) -> Result<(i32, i32), String> {
        let x = scale_axis(point.0, self.pointer_space.0);
        let y = scale_axis(point.1, self.pointer_space.1);
        self.command(MOVE_ABSOLUTE, x, y)?;
        Ok(point)
    }

    pub fn button(&mut self, code: u16, pressed: bool) -> Result<(), String> {
        self.command(BUTTON, i32::from(code), i32::from(pressed))
    }

    pub fn wheel(&mut self, horizontal: bool, amount: i32) -> Result<(), String> {
        self.command(WHEEL, i32::from(horizontal), amount)
    }

    pub fn key(&mut self, code: u16, pressed: bool) -> Result<(), String> {
        self.command(KEY, i32::from(code), i32::from(pressed))
    }

    pub fn click(&mut self, code: u16) -> Result<(), String> {
        self.button(code, true)?;
        thread::sleep(CLICK_HOLD);
        self.button(code, false)
    }

    pub fn chord(&mut self, modifiers: &[u16], key: u16) -> Result<(), String> {
        for modifier in modifiers {
            self.key(*modifier, true)?;
            thread::sleep(Duration::from_millis(12));
        }
        self.key(key, true)?;
        thread::sleep(Duration::from_millis(24));
        self.key(key, false)?;
        thread::sleep(Duration::from_millis(24));
        for modifier in modifiers.iter().rev() {
            self.key(*modifier, false)?;
            thread::sleep(Duration::from_millis(12));
        }
        Ok(())
    }

    fn command(&mut self, opcode: u8, first: i32, second: i32) -> Result<(), String> {
        let mut packet = [0_u8; COMMAND_BYTES];
        packet[0] = opcode;
        packet[1..5].copy_from_slice(&first.to_le_bytes());
        packet[5..9].copy_from_slice(&second.to_le_bytes());
        let input = self
            .input
            .as_mut()
            .ok_or("kernel virtual input is already closed")?;
        input
            .write_all(&packet)
            .and_then(|()| input.flush())
            .map_err(|error| format!("send kernel virtual input command: {error}"))?;
        let mut status = [0_u8; 1];
        self.output
            .read_exact(&mut status)
            .map_err(|error| format!("read kernel virtual input acknowledgement: {error}"))?;
        match status[0] {
            0 => Ok(()),
            value => Err(format!(
                "kernel virtual input rejected command {opcode} with status {value}"
            )),
        }
    }

    pub fn shutdown(&mut self) -> Result<(), String> {
        if self.input.is_some() {
            let _ = self.command(SHUTDOWN, 0, 0);
            self.input.take();
        }
        let status = self
            .child
            .wait()
            .map_err(|error| format!("wait for kernel virtual input process: {error}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("kernel virtual input process exited with {status}"))
        }
    }
}

impl Drop for NativeInput {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn scale_axis(value: i32, extent: i32) -> i32 {
    let value = value.clamp(0, extent.saturating_sub(1));
    i64::from(value)
        .saturating_mul(i64::from(AXIS_MAX))
        .checked_div(i64::from(extent.saturating_sub(1)))
        .and_then(|value| i32::try_from(value).ok())
        .unwrap_or(0)
}

pub fn run_device_process(args: &[String]) -> Result<(), String> {
    let seat_name = argument(args, "--seat").ok_or("native-input requires --seat")?;
    validate_seat_name(seat_name)?;
    let mut devices = Devices::create(seat_name)
        .map_err(|error| format!("create isolated uinput devices: {error}"))?;
    thread::sleep(DEVICE_SETTLE);
    io::stdout()
        .write_all(&[READY[0], READY[1], READY[2], READY[3], VERSION])
        .and_then(|()| io::stdout().flush())
        .map_err(|error| format!("write uinput ready handshake: {error}"))?;

    let mut command = [0_u8; COMMAND_BYTES];
    loop {
        match io::stdin().read_exact(&mut command) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(error) => return Err(format!("read uinput command: {error}")),
        }
        let first = i32::from_le_bytes(command[1..5].try_into().expect("fixed command"));
        let second = i32::from_le_bytes(command[5..9].try_into().expect("fixed command"));
        let result = devices.execute(command[0], first, second);
        io::stdout()
            .write_all(&[u8::from(result.is_err())])
            .and_then(|()| io::stdout().flush())
            .map_err(|error| format!("write uinput acknowledgement: {error}"))?;
        result.map_err(|error| format!("emit uinput event: {error}"))?;
        if command[0] == SHUTDOWN {
            return Ok(());
        }
    }
}

struct Devices {
    pointer: VirtualDevice,
    keyboard: VirtualDevice,
}

impl Devices {
    fn create(seat_name: &str) -> io::Result<Self> {
        let pointer_keys =
            AttributeSet::from_iter([KeyCode::BTN_LEFT, KeyCode::BTN_MIDDLE, KeyCode::BTN_RIGHT]);
        let relative_axes = AttributeSet::from_iter([
            RelativeAxisCode::REL_X,
            RelativeAxisCode::REL_Y,
            RelativeAxisCode::REL_WHEEL,
            RelativeAxisCode::REL_HWHEEL,
        ]);
        let properties = AttributeSet::from_iter([PropType::POINTER]);
        let absolute = AbsInfo::new(0, 0, AXIS_MAX, 0, 0, 1);
        let pointer = VirtualDevice::builder()?
            .name(&device_name(seat_name, "Pointer"))
            .with_properties(&properties)?
            .with_keys(&pointer_keys)?
            .with_relative_axes(&relative_axes)?
            .with_absolute_axis(&UinputAbsSetup::new(AbsoluteAxisCode::ABS_X, absolute))?
            .with_absolute_axis(&UinputAbsSetup::new(AbsoluteAxisCode::ABS_Y, absolute))?
            .build()?;

        let keyboard_keys = AttributeSet::from_iter((1..=255).map(KeyCode::new));
        let keyboard = VirtualDevice::builder()?
            .name(&device_name(seat_name, "Keyboard"))
            .with_keys(&keyboard_keys)?
            .build()?;
        Ok(Self { pointer, keyboard })
    }

    fn execute(&mut self, opcode: u8, first: i32, second: i32) -> io::Result<()> {
        match opcode {
            MOVE_ABSOLUTE => self.pointer.emit(&[
                InputEvent::new(
                    EventType::ABSOLUTE.0,
                    AbsoluteAxisCode::ABS_X.0,
                    first.clamp(0, AXIS_MAX),
                ),
                InputEvent::new(
                    EventType::ABSOLUTE.0,
                    AbsoluteAxisCode::ABS_Y.0,
                    second.clamp(0, AXIS_MAX),
                ),
            ]),
            MOVE_RELATIVE => self.pointer.emit(&[
                InputEvent::new(EventType::RELATIVE.0, RelativeAxisCode::REL_X.0, first),
                InputEvent::new(EventType::RELATIVE.0, RelativeAxisCode::REL_Y.0, second),
            ]),
            BUTTON => self.pointer.emit(&[InputEvent::new(
                EventType::KEY.0,
                u16::try_from(first).unwrap_or_default(),
                i32::from(second != 0),
            )]),
            WHEEL => self.pointer.emit(&[InputEvent::new(
                EventType::RELATIVE.0,
                if first == 0 {
                    RelativeAxisCode::REL_WHEEL.0
                } else {
                    RelativeAxisCode::REL_HWHEEL.0
                },
                second,
            )]),
            KEY => self.keyboard.emit(&[InputEvent::new(
                EventType::KEY.0,
                u16::try_from(first).unwrap_or_default(),
                i32::from(second != 0),
            )]),
            SHUTDOWN => Ok(()),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unknown uinput command",
            )),
        }
    }
}

fn argument<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1].as_str())
}

fn device_name(seat_name: &str, kind: &str) -> String {
    format!("COSMIC Isolated {seat_name} {kind}")
}

fn validate_seat_name(seat_name: &str) -> Result<(), String> {
    if seat_name.is_empty()
        || !seat_name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(format!("invalid isolated seat name `{seat_name}`"));
    }
    for kind in ["Pointer", "Keyboard"] {
        let name = device_name(seat_name, kind);
        if name.len() > UINPUT_NAME_MAX_BYTES {
            return Err(format!(
                "isolated seat name is too long for a uinput {kind} device"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_coordinates_are_bounded_and_monotonic() {
        assert_eq!(scale_axis(-1, DEFAULT_POINTER_SPACE.0), 0);
        assert_eq!(scale_axis(0, DEFAULT_POINTER_SPACE.0), 0);
        assert!(
            scale_axis(1_200, DEFAULT_POINTER_SPACE.0) > scale_axis(600, DEFAULT_POINTER_SPACE.0)
        );
        assert_eq!(scale_axis(i32::MAX, DEFAULT_POINTER_SPACE.0), AXIS_MAX);
    }

    #[test]
    fn isolated_device_names_are_exact_and_bounded() {
        let seat = "cosmic-isolated-background-launch-1234-7";
        validate_seat_name(seat).unwrap();
        assert_eq!(
            device_name(seat, "Pointer"),
            "COSMIC Isolated cosmic-isolated-background-launch-1234-7 Pointer"
        );
        assert!(validate_seat_name("physical seat").is_err());
        assert!(validate_seat_name(&"x".repeat(UINPUT_NAME_MAX_BYTES)).is_err());
    }
}
