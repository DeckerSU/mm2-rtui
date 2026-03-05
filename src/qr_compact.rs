// Compact QR code rendering using UTF-8 block characters
// This renders two rows of QR code pixels into one terminal line using ▀▄█ characters

use qrcode::QrCode;

/// Renders a QR code in compact format using UTF-8 block characters.
/// Each terminal line represents two rows of the QR code matrix.
/// 
/// Characters used (inverted - white background, black foreground):
/// - ' ' (space) when both pixels are white (background)
/// - '▄' (lower half block) when top is white, bottom is black
/// - '▀' (upper half block) when top is black, bottom is white
/// - '█' (full block) when both pixels are black
pub fn render_qr_compact(data: &str) -> Result<Vec<String>, qrcode::types::QrError> {
    let code = QrCode::new(data.as_bytes())?;
    let matrix = code.to_colors();
    let width = code.width();
    
    let mut lines = Vec::new();
    
    // Add top border (2 lines of spaces for white background)
    let border_line = " ".repeat(width + 4);
    lines.push(border_line.clone());
    lines.push(border_line.clone());
    
    // Process matrix two rows at a time
    let mut row = 0;
    while row < width {
        let mut line = String::from("  "); // Left border (white)
        
        for col in 0..width {
            let top_pixel = matrix[row * width + col];
            let bottom_pixel = if row + 1 < width {
                matrix[(row + 1) * width + col]
            } else {
                qrcode::types::Color::Light // Treat out-of-bounds as white
            };
            
            // Inverted logic: Dark = black module, Light = white background
            let ch = match (top_pixel, bottom_pixel) {
                (qrcode::types::Color::Light, qrcode::types::Color::Light) => ' ',  // Both white
                (qrcode::types::Color::Light, qrcode::types::Color::Dark) => '▄',   // Top white, bottom black
                (qrcode::types::Color::Dark, qrcode::types::Color::Light) => '▀',   // Top black, bottom white
                (qrcode::types::Color::Dark, qrcode::types::Color::Dark) => '█',    // Both black
            };
            
            line.push(ch);
        }
        
        line.push_str("  "); // Right border (white)
        lines.push(line);
        
        row += 2; // Skip two rows since we processed them together
    }
    
    // Add bottom border (2 lines of spaces for white background)
    lines.push(border_line.clone());
    lines.push(border_line);
    
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_render_qr_compact() {
        let result = render_qr_compact("RBPG3zECm1SBEeFLCvqvPfGPnk5HXXhxV1");
        assert!(result.is_ok());
        
        let lines = result.unwrap();
        assert!(!lines.is_empty());
        
        // Print for visual inspection
        for line in &lines {
            println!("{}", line);
        }
    }
}
