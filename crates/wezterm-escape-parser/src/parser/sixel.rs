use crate::color::RgbColor;
use crate::{Sixel, SixelData};

const MAX_PARAMS: usize = 5;
const MAX_SIXEL_SIZE: usize = 100_000_000;
/// Upper bound on the number of decoded sixel commands kept for one image.
/// Each entry is 8 bytes and paints at most a 1x6 pixel column per color,
/// while the terminal refuses images over 25M pixels, so real images stay
/// far below this; without it an unterminated `ESC P q` stream grows memory
/// at 8x its input with no limit.
pub(crate) const MAX_SIXEL_DATA_LEN: usize = 32 * 1024 * 1024;

pub struct SixelBuilder {
    pub sixel: Sixel,
    params: [i64; MAX_PARAMS],
    param_no: usize,
    current_command: u8,
    overflowed: bool,
}

impl SixelBuilder {
    pub fn new(params: &[i64]) -> Self {
        let pan = match params.get(0).unwrap_or(&0) {
            7 | 8 | 9 => 1,
            0 | 1 | 5 | 6 => 2,
            3 | 4 => 3,
            2 => 5,
            _ => 2,
        };
        let background_is_transparent = match params.get(1).unwrap_or(&0) {
            1 => true,
            _ => false,
        };
        let horizontal_grid_size = params.get(2).map(|&x| x);

        Self {
            sixel: Sixel {
                pan,
                pad: 1,
                pixel_width: None,
                pixel_height: None,
                background_is_transparent,
                horizontal_grid_size,
                data: vec![],
            },
            param_no: 0,
            params: [-1; MAX_PARAMS],
            current_command: 0,
            overflowed: false,
        }
    }

    /// True once the image exceeded `MAX_SIXEL_DATA_LEN`; the caller must
    /// drop it instead of rendering a truncated picture.
    pub fn overflowed(&self) -> bool {
        self.overflowed
    }

    pub fn push(&mut self, data: u8) {
        if self.overflowed {
            return;
        }
        if self.sixel.data.len() >= MAX_SIXEL_DATA_LEN {
            log::error!(
                "Ignoring sixel image because it exceeds {} data entries",
                MAX_SIXEL_DATA_LEN
            );
            self.overflowed = true;
            self.sixel.data = vec![];
            return;
        }
        match data {
            b'$' => {
                self.finish_command();
                self.sixel.data.push(SixelData::CarriageReturn);
            }
            b'-' => {
                self.finish_command();
                self.sixel.data.push(SixelData::NewLine);
            }
            0x3f..=0x7e if self.current_command == b'!' => {
                self.sixel.data.push(SixelData::Repeat {
                    repeat_count: self.params[0] as u32,
                    data: data - 0x3f,
                });
                self.finish_command();
            }
            0x3f..=0x7e => {
                self.finish_command();
                self.sixel.data.push(SixelData::Data(data - 0x3f));
            }
            b'#' | b'!' | b'"' => {
                self.finish_command();
                self.current_command = data;
            }
            b'0'..=b'9' if self.current_command != 0 => {
                let pos = self.param_no;
                if pos >= MAX_PARAMS {
                    return;
                }
                if self.params[pos] == -1 {
                    self.params[pos] = 0;
                }
                self.params[pos] = self.params[pos]
                    .saturating_mul(10)
                    .saturating_add((data - b'0') as i64);
            }
            b';' if self.current_command != 0 => {
                let pos = self.param_no;
                if pos >= MAX_PARAMS {
                    return;
                }
                self.param_no += 1;
            }
            _ => {
                // Invalid; break out of current command
                self.finish_command();
            }
        }
    }

    fn finish_command(&mut self) {
        match self.current_command {
            b'#' if self.param_no >= 4 => {
                // Define a color
                let color_number = self.params[0] as u16;
                let system = self.params[1] as u16;
                let a = self.params[2] as u16;
                let b = self.params[3] as u8;
                let c = self.params[4] as u8;

                if system == 1 {
                    self.sixel.data.push(SixelData::DefineColorMapHSL {
                        color_number,
                        hue_angle: a,
                        lightness: b,
                        saturation: c,
                    });
                } else {
                    let r = a as f32 * 255.0 / 100.;
                    let g = b as f32 * 255.0 / 100.;
                    let b = c as f32 * 255.0 / 100.;
                    let rgb = RgbColor::new_8bpc(r as u8, g as u8, b as u8); // FIXME: from linear
                    self.sixel
                        .data
                        .push(SixelData::DefineColorMapRGB { color_number, rgb });
                }
            }
            b'#' => {
                // Use a color
                let color_number = self.params[0] as u16;

                self.sixel
                    .data
                    .push(SixelData::SelectColorMapEntry(color_number));
            }
            b'"' => {
                // Set raster attributes
                let pan = if self.params[0] == -1 {
                    2
                } else {
                    self.params[0]
                };
                let pad = if self.params[1] == -1 {
                    1
                } else {
                    self.params[1]
                };
                let pixel_width = self.params[2];
                let pixel_height = self.params[3];

                self.sixel.pan = pan;
                self.sixel.pad = pad;

                if self.param_no >= 3 {
                    self.sixel.pixel_width.replace(pixel_width as u32);
                    self.sixel.pixel_height.replace(pixel_height as u32);

                    let (size, overflow) =
                        (pixel_width as usize).overflowing_mul(pixel_height as usize);

                    // Ideally we'd just use `try_reserve` here, but that is
                    // nightly Rust only at the time of writing this comment:
                    // <https://github.com/rust-lang/rust/issues/48043>
                    if size > MAX_SIXEL_SIZE || overflow {
                        log::error!(
                            "Ignoring sixel data {}x{} because {} bytes \
                             either overflows or exceeds the max allowed {}",
                            pixel_width,
                            pixel_height,
                            size,
                            MAX_SIXEL_SIZE
                        );
                        self.sixel.pixel_width = None;
                        self.sixel.pixel_height = None;
                        self.sixel.data.clear();
                        return;
                    }
                    // `size` counts pixels while each entry covers a 1x6
                    // column, so never pre-allocate past the data cap.
                    self.sixel.data.reserve(size.min(MAX_SIXEL_DATA_LEN));
                }
            }
            _ => {}
        }
        self.param_no = 0;
        self.params = [-1; MAX_PARAMS];
        self.current_command = 0;
    }

    pub fn finish(&mut self) {
        self.finish_command();
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::parser::Parser;
    use crate::{Action, Esc, EscCode};
    use alloc::boxed::Box;
    use k9::assert_equal as assert_eq;

    #[test]
    fn sixel() {
        let mut p = Parser::new();
        let actions = p.parse_as_vec(b"\x1bP1;2;3;q@\x1b\\");
        assert_eq!(
            vec![
                Action::Sixel(Box::new(Sixel {
                    pan: 2,
                    pad: 1,
                    pixel_width: None,
                    pixel_height: None,
                    background_is_transparent: false,
                    horizontal_grid_size: Some(3),
                    data: vec![SixelData::Data(1)]
                })),
                Action::Esc(Esc::Code(EscCode::StringTerminator)),
            ],
            actions
        );

        assert_eq!(format!("{}", actions[0]), "\x1bP0;0;3q@");

        // This is the "HI" example from wikipedia
        let mut p = Parser::new();
        let actions = p.parse_as_vec(
            b"\x1bPq\
        #0;2;0;0;0#1;2;100;100;0#2;2;0;100;0\
        #1~~@@vv@@~~@@~~$\
        #2??}}GG}}??}}??-\
        #1!14@\
        \x1b\\",
        );

        assert_eq!(
            format!("{}", actions[0]),
            "\x1bP0;0q\
        #0;2;0;0;0#1;2;100;100;0#2;2;0;100;0\
        #1~~@@vv@@~~@@~~$\
        #2??}}GG}}??}}??-\
        #1!14@"
        );

        use SixelData::*;
        assert_eq!(
            vec![
                Action::Sixel(Box::new(Sixel {
                    pan: 2,
                    pad: 1,
                    pixel_width: None,
                    pixel_height: None,
                    background_is_transparent: false,
                    horizontal_grid_size: None,
                    data: vec![
                        DefineColorMapRGB {
                            color_number: 0,
                            rgb: RgbColor::new_8bpc(0, 0, 0)
                        },
                        DefineColorMapRGB {
                            color_number: 1,
                            rgb: RgbColor::new_8bpc(255, 255, 0)
                        },
                        DefineColorMapRGB {
                            color_number: 2,
                            rgb: RgbColor::new_8bpc(0, 255, 0)
                        },
                        SelectColorMapEntry(1),
                        Data(63),
                        Data(63),
                        Data(1),
                        Data(1),
                        Data(55),
                        Data(55),
                        Data(1),
                        Data(1),
                        Data(63),
                        Data(63),
                        Data(1),
                        Data(1),
                        Data(63),
                        Data(63),
                        CarriageReturn,
                        SelectColorMapEntry(2),
                        Data(0),
                        Data(0),
                        Data(62),
                        Data(62),
                        Data(8),
                        Data(8),
                        Data(62),
                        Data(62),
                        Data(0),
                        Data(0),
                        Data(62),
                        Data(62),
                        Data(0),
                        Data(0),
                        NewLine,
                        SelectColorMapEntry(1),
                        Repeat {
                            repeat_count: 14,
                            data: 1
                        }
                    ]
                })),
                Action::Esc(Esc::Code(EscCode::StringTerminator)),
            ],
            actions
        );
    }

    fn count_sixels(p: &mut Parser, bytes: &[u8]) -> usize {
        let mut n = 0;
        p.parse(bytes, |a| {
            if matches!(a, Action::Sixel(_)) {
                n += 1;
            }
        });
        n
    }

    #[test]
    fn oversized_sixel_is_dropped_and_parser_recovers() {
        let mut p = Parser::new();
        let mut sixels = count_sixels(&mut p, b"\x1bPq");
        let chunk = vec![b'?'; 1 << 20];
        for _ in 0..(MAX_SIXEL_DATA_LEN / chunk.len() + 1) {
            sixels += count_sixels(&mut p, &chunk);
        }
        sixels += count_sixels(&mut p, b"\x1b\\");
        assert_eq!(sixels, 0, "an image past the data cap must not be emitted");

        let actions = p.parse_as_vec(b"\x1bPq@\x1b\\x");
        assert_eq!(actions.len(), 3, "{actions:?}");
        assert!(matches!(&actions[0], Action::Sixel(s) if s.data == vec![SixelData::Data(1)]));
        assert_eq!(actions[2], Action::Print('x'));
    }

    #[test]
    fn raster_attributes_do_not_reserve_past_the_data_cap() {
        let mut builder = SixelBuilder::new(&[]);
        for &b in b"\"1;1;10000;10000" {
            builder.push(b);
        }
        builder.finish();
        assert!(builder.sixel.data.capacity() <= MAX_SIXEL_DATA_LEN);
    }
}
