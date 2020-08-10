static SERIAL_PORT: Lock<Option<SerialPort>> = Lock::new(None);

macro_rules! print {
    ($($arg: tt)*,) => {
    }
}
