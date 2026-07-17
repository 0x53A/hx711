#![no_std]

use defmt::error;
use embassy_time::{Delay, Duration, Instant, Timer};
use embedded_hal::delay::DelayNs;
use embedded_hal::digital::{InputPin, OutputPin};

pub const RAW_MIN: i32 = -8_388_608;
pub const RAW_MAX: i32 = 8_388_607;

/// Note: the channel selection always affects the *next* read operation
#[allow(unused)]
#[derive(Clone, Copy)]
pub enum Hx711Channel {
    A128, // Channel A, Gain 128 (25 pulses)
    B32,  // Channel B, Gain 32 (26 pulses)
    A64,  // Channel A, Gain 64 (27 pulses)
}

impl Hx711Channel {
    fn pulse_count(&self) -> u8 {
        match self {
            Hx711Channel::A128 => 25,
            Hx711Channel::B32 => 26,
            Hx711Channel::A64 => 27,
        }
    }
}

pub struct Hx711<CLK, DAT> {
    clk: CLK,
    dat: DAT,
}

pub struct Hx711TimeoutError;

impl<CLK, DAT> Hx711<CLK, DAT>
where
    CLK: OutputPin,
    DAT: InputPin,
{
    /// Initialize the HX711 with clock and data pins
    pub fn new(clk: CLK, dat: DAT) -> Self {
        let mut hx711 = Self { clk, dat };

        // Ensure clock starts low
        hx711.clk.set_low().unwrap();
        hx711
    }

    pub fn is_data_ready(&mut self) -> bool {
        self.dat.is_low().unwrap()
    }

    pub async fn wait_for_data_ready(&mut self) -> Result<(), Hx711TimeoutError> {
        let start_time = Instant::now();
        while !self.is_data_ready() {
            embassy_futures::yield_now().await;
            if start_time.elapsed() > Duration::from_millis(150) {
                return Err(Hx711TimeoutError);
            }
        }
        Ok(())
    }

    /// Read a raw 24-bit value from the HX711 from channel A128
    pub async fn read_value(&mut self) -> Result<i32, Hx711TimeoutError> {
        self.read_value_with_channel(Hx711Channel::A128).await
    }

    /// Read a raw 24-bit value from the HX711 with specified channel/gain
    pub async fn read_value_with_channel(
        &mut self,
        channel: Hx711Channel,
    ) -> Result<i32, Hx711TimeoutError> {
        self.wait_for_data_ready().await?;

        let mut data: u32 = 0;

        // Read 24 bits of data
        for _i in 0..24 {
            self.clk.set_high().unwrap();
            self.delay_us(1); // Minimum 0.2µs according to datasheet

            // Read the data bit
            let current_bit = if self.dat.is_high().unwrap() { 1 } else { 0 };
            data = (data << 1) | current_bit;

            self.clk.set_low().unwrap();
            self.delay_us(1); // Minimum 0.2µs according to datasheet
        }

        // Send additional pulses to set the gain/channel for next conversion
        let pulse_count = channel.pulse_count();
        for _ in 24..pulse_count {
            self.clk.set_high().unwrap();
            self.delay_us(1);
            self.clk.set_low().unwrap();
            self.delay_us(1);
        }

        // Convert 24-bit two's complement to signed 32-bit
        let result = if data & 0x800000 != 0 {
            // Negative number: sign extend
            (data | 0xFF000000) as i32
        } else {
            // Positive number
            data as i32
        };

        // Verify DOUT goes high after reading
        self.delay_us(1);
        if !self.dat.is_high().unwrap() {
            error!("Data line did not return high after reading");
        }

        Ok(result)
    }

    /// Put HX711 into power down mode
    pub async fn power_down(&mut self) {
        // datasheet: chip enters sleep if PD_SCK is pulled high for >= 60us
        self.clk.set_high().unwrap();
        Timer::after(Duration::from_micros(100)).await;
    }

    /// Wake up HX711 from power down mode
    pub async fn power_up(&mut self) {
        self.clk.set_low().unwrap();
        Timer::after(Duration::from_millis(1)).await;
    }

    /// Reset the HX711
    pub async fn reset(&mut self) {
        self.power_down().await;
        self.power_up().await;
    }

    /// Microsecond delay
    fn delay_us(&self, us: u32) {
        let mut delay = Delay;
        delay.delay_us(us);
    }
}

pub fn is_saturated_raw(raw: i32) -> bool {
    raw == RAW_MIN || raw == RAW_MAX
}
