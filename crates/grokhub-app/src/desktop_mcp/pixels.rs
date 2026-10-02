//! Pure pixel helpers shared by the X11 capture path.

/// Unpack a ZPixmap into RGBA. Masks select channels. `msb_first` is the server byte order.
#[cfg(any(unix, test))]
pub(crate) fn bgrx_to_rgba(
    src: &[u8],
    width: u32,
    height: u32,
    bits_per_pixel: u8,
    msb_first: bool,
    red_mask: u32,
    green_mask: u32,
    blue_mask: u32,
) -> Result<Vec<u8>, String> {
    let bpp = (bits_per_pixel / 8) as usize;
    if bpp != 3 && bpp != 4 {
        return Err(format!("unsupported screenshot depth {bits_per_pixel}"));
    }
    let need = (width as usize)
        .saturating_mul(height as usize)
        .saturating_mul(bpp);
    if src.len() < need {
        return Err("screenshot is shorter than its geometry".into());
    }
    let mut out = vec![0u8; (width as usize) * (height as usize) * 4];
    for i in 0..(width as usize) * (height as usize) {
        let px = &src[i * bpp..i * bpp + bpp];
        let mut word = 0u32;
        if msb_first {
            for (shift, byte) in px.iter().rev().enumerate() {
                word |= (*byte as u32) << (shift * 8);
            }
        } else {
            for (shift, byte) in px.iter().enumerate() {
                word |= (*byte as u32) << (shift * 8);
            }
        }
        let di = i * 4;
        out[di] = channel(word, red_mask);
        out[di + 1] = channel(word, green_mask);
        out[di + 2] = channel(word, blue_mask);
        out[di + 3] = 255;
    }
    Ok(out)
}

#[cfg(any(unix, test))]
fn channel(pixel: u32, mask: u32) -> u8 {
    if mask == 0 {
        return 0;
    }
    let shift = mask.trailing_zeros();
    let bits = mask.count_ones();
    let raw = (pixel & mask) >> shift;
    if bits >= 8 {
        (raw >> (bits - 8)) as u8
    } else if bits == 0 {
        0
    } else {
        ((raw * 255) / ((1u32 << bits) - 1)) as u8
    }
}

/// Copy a rectangle out of a tightly packed RGBA buffer. Out-of-range pixels stay transparent.
#[cfg(any(unix, test))]
pub(crate) fn crop_rgba(
    src: &[u8],
    src_w: u32,
    src_h: u32,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
) -> Result<Vec<u8>, String> {
    let need = (src_w as usize).saturating_mul(src_h as usize).saturating_mul(4);
    if src.len() < need {
        return Err("crop source is shorter than its geometry".into());
    }
    let mut out = vec![0u8; (w as usize) * (h as usize) * 4];
    for row in 0..h {
        let sy = y.saturating_add(row as i32);
        if sy < 0 || sy >= src_h as i32 {
            continue;
        }
        for col in 0..w {
            let sx = x.saturating_add(col as i32);
            if sx < 0 || sx >= src_w as i32 {
                continue;
            }
            let si = (sy as usize * src_w as usize + sx as usize) * 4;
            let di = (row as usize * w as usize + col as usize) * 4;
            out[di..di + 4].copy_from_slice(&src[si..si + 4]);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_mcp_bgrx_little_endian_is_rgba() {
        let src = [1u8, 2, 3, 4];
        let rgba = bgrx_to_rgba(&src, 1, 1, 32, false, 0x00ff_0000, 0x0000_ff00, 0x0000_00ff)
            .unwrap();
        assert_eq!(rgba, vec![3, 2, 1, 255]);
    }

    #[test]
    fn desktop_mcp_bgrx_big_endian_and_short_buffer() {
        let src = [0u8, 3, 2, 1];
        let rgba = bgrx_to_rgba(&src, 1, 1, 32, true, 0x00ff_0000, 0x0000_ff00, 0x0000_00ff)
            .unwrap();
        assert_eq!(rgba, vec![3, 2, 1, 255]);
        assert!(bgrx_to_rgba(&[0, 0], 1, 1, 32, false, 1, 1, 1).is_err());
        assert!(bgrx_to_rgba(&[0, 0], 1, 1, 16, false, 1, 1, 1).is_err());
    }

    #[test]
    fn desktop_mcp_crop_rgba() {
        let mut src = vec![0u8; 4 * 4];
        src[0] = 9;
        src[4] = 8;
        src[8] = 7;
        src[12] = 6;
        let cropped = crop_rgba(&src, 2, 2, 1, 0, 1, 2).unwrap();
        assert_eq!(cropped.len(), 8);
        assert_eq!(cropped[0], 8);
        assert_eq!(cropped[4], 6);
        let edge = crop_rgba(&src, 2, 2, -1, -1, 2, 2).unwrap();
        assert_eq!(edge[0], 0);
        let x = 1usize;
        let y = 1usize;
        let w = 2usize;
        assert_eq!(edge[(y * w + x) * 4], 9);
        assert!(crop_rgba(&[1, 2, 3], 2, 2, 0, 0, 1, 1).is_err());
    }
}
