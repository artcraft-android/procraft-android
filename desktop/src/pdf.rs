//! A PDF's pages drawn by the system (Windows' own PDF renderer), to bring one in as an artwork:
//! each page a PNG on white, fitted into the first page's size.

use std::path::Path;

/// The resolution a PDF's pages are drawn at.
pub const DPI: f32 = 300.0;
/// The most a canvas side made from a PDF may be (pixels).
#[cfg(windows)]
const MOST_SIDE: f32 = 4096.0;

/// Draws the PDF at `path` into the folder `pages` as `page-N.png` (N from 1), each on white,
/// fitted into the middle of the first page's size at [`DPI`] (its longer side within 4096):
/// the number of pages and that size. None are drawn if a canvas that size holds fewer layers.
#[cfg(windows)]
pub fn draw_pages(path: &Path, pages: &Path) -> Result<(u32, u32, u32), Box<dyn std::error::Error>> {
    use windows::Data::Pdf::{PdfDocument, PdfPage, PdfPageRenderOptions};
    use windows::Storage::StorageFile;
    use windows::Storage::Streams::{DataReader, InMemoryRandomAccessStream};
    use windows::UI::Color;
    use windows::core::HSTRING;

    // the system wants a whole path, with backslashes
    let whole = std::path::absolute(path)?.to_string_lossy().replace('/', "\\");
    let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(whole))?.join()?;
    let pdf = PdfDocument::LoadFromFileAsync(&file)?.join()?;
    let count = pdf.PageCount()?;
    // a page's size in points (the system gives it at 96 to the inch, a PDF's at 72)
    let points = |page: &PdfPage| -> windows::core::Result<(f32, f32)> {
        let size = page.Size()?;
        Ok((size.Width * 0.75, size.Height * 0.75))
    };
    let (first_w, first_h) = points(&pdf.GetPage(0)?)?;
    let scale = (DPI / 72.0).min(MOST_SIDE / first_w.max(first_h).max(1.0));
    let (w, h) = (((first_w * scale).round() as u32).max(1), ((first_h * scale).round() as u32).max(1));
    // more pages than a canvas this size holds as layers: none drawn, as none will be taken
    if count > procraft_core::layers_for(w, h) {
        return Ok((count, w, h));
    }
    for i in 0..count {
        let page = pdf.GetPage(i)?;
        let (pw, ph) = points(&page)?;
        let fit = (w as f32 / pw.max(1.0)).min(h as f32 / ph.max(1.0));
        let (dw, dh) = (((pw * fit).round() as u32).clamp(1, w), ((ph * fit).round() as u32).clamp(1, h));
        let options = PdfPageRenderOptions::new()?;
        options.SetDestinationWidth(dw)?;
        options.SetDestinationHeight(dh)?;
        options.SetBackgroundColor(Color { A: 255, R: 255, G: 255, B: 255 })?;
        let stream = InMemoryRandomAccessStream::new()?;
        page.RenderWithOptionsToStreamAsync(&stream, &options)?.join()?;
        let size = u32::try_from(stream.Size()?)?;
        let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0)?)?;
        reader.LoadAsync(size)?.join()?;
        let mut png = vec![0u8; size as usize];
        reader.ReadBytes(&mut png)?;
        // in the middle of the canvas, on white
        let mut drawn = image::load_from_memory_with_format(&png, image::ImageFormat::Png)?.to_rgba8();
        // the system draws at the screen's scale (1.4 times as many pixels at 140%): brought to
        // the size asked for
        if drawn.dimensions() != (dw, dh) {
            drawn = image::imageops::resize(&drawn, dw, dh, image::imageops::FilterType::CatmullRom);
        }
        let mut canvas = image::RgbaImage::from_pixel(w, h, image::Rgba([255, 255, 255, 255]));
        let (x, y) = (w.saturating_sub(drawn.width()) / 2, h.saturating_sub(drawn.height()) / 2);
        image::imageops::overlay(&mut canvas, &drawn, i64::from(x), i64::from(y));
        canvas.save(pages.join(format!("page-{}.png", i + 1)))?;
    }
    Ok((count, w, h))
}

/// PDFs are drawn by Windows' renderer: elsewhere they can't be brought in yet.
#[cfg(not(windows))]
pub fn draw_pages(_path: &Path, _pages: &Path) -> Result<(u32, u32, u32), Box<dyn std::error::Error>> {
    Err("no PDF renderer on this system".into())
}
