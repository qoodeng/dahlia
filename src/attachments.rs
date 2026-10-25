use image::{ImageFormat, ImageReader};
use markraft_commonmark::{SourceDocument, commonmark_schema};
use markraft_core::{Fragment, MarkSet, Node};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Cursor},
    path::{Component, Path, PathBuf},
};

const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_PIXELS: u64 = 32 * 1024 * 1024;
const MAX_IMAGES: usize = 64;
const MAX_ASSET_BYTES: usize = 64 * 1024 * 1024;

fn retain(images: &mut BTreeMap<String, Vec<u8>>, bytes: Vec<u8>) -> io::Result<String> {
    let relative = reference(&bytes);
    if !images.contains_key(&relative) {
        if images.len() >= MAX_IMAGES
            || images.values().map(Vec::len).sum::<usize>() + bytes.len() > MAX_ASSET_BYTES
        {
            return Err(io::Error::other("Add up to 64 images (64 MB) at a time."));
        }
        images.insert(relative.clone(), bytes);
    }
    Ok(relative)
}

pub fn read_image(path: &Path) -> io::Result<Vec<u8>> {
    if !path
        .symlink_metadata()
        .map_err(|error| {
            io::Error::other(format!(
                "{}: {}",
                crate::copy::file(path),
                crate::copy::plain(&error)
            ))
        })?
        .file_type()
        .is_file()
    {
        return Err(io::Error::other(format!(
            "{} isn’t an image.",
            crate::copy::file(path)
        )));
    }
    let bytes = crate::transfer::read_bounded(path, MAX_BYTES).map_err(|error| {
        io::Error::other(format!(
            "{}: {}",
            crate::copy::file(path),
            crate::copy::plain(&error)
        ))
    })?;
    normalize(&bytes)
        .map_err(|error| io::Error::other(format!("{}: {error}", crate::copy::file(path))))
}
pub fn normalize(bytes: &[u8]) -> io::Result<Vec<u8>> {
    if bytes.len() > MAX_BYTES {
        return Err(io::Error::other("An image is larger than 16 MB. Choose a smaller one."));
    }
    let result = normalize_rust(bytes);
    #[cfg(target_os = "macos")]
    return result.or_else(|rust_error| {
        native_image::reencode(bytes, "public.png").map_err(|native_error| {
            log::warn!("Image not decoded: {rust_error}; {native_error}");
            io::Error::other("Dahlia can’t read this image format.")
        })
    });
    #[cfg(not(target_os = "macos"))]
    result
}
fn validate_dimensions(width: u64, height: u64) -> io::Result<()> {
    if width == 0
        || height == 0
        || width > 16_000
        || height > 16_000
        || width
            .checked_mul(height)
            .is_none_or(|pixels| pixels > MAX_PIXELS)
    {
        return Err(io::Error::other(
            "An image is too large (over 32 megapixels or 16,000 pixels on a side). Choose a smaller one.",
        ));
    }
    Ok(())
}
fn normalize_rust(bytes: &[u8]) -> io::Result<Vec<u8>> {
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(io::Error::other)?;
    let (width, height) = reader.into_dimensions().map_err(io::Error::other)?;
    validate_dimensions(width as u64, height as u64)?;
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(io::Error::other)?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().map_err(io::Error::other)?;
    let mut output = Cursor::new(Vec::new());
    image
        .write_to(&mut output, ImageFormat::Png)
        .map_err(io::Error::other)?;
    if output.get_ref().len() > MAX_BYTES {
        return Err(io::Error::other(
            "An image is too large after conversion (over 16 MB). Choose a smaller one.",
        ));
    }
    Ok(output.into_inner())
}
pub fn reference(bytes: &[u8]) -> String {
    format!(
        "attachments/{}.png",
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}

/// Immutable content-addressed assets are retained for undo, history and recovery.
pub fn publish(base: &Path, images: &BTreeMap<String, Vec<u8>>) -> io::Result<()> {
    if images.is_empty() {
        return Ok(());
    }
    crate::transfer::safe_directory(base)?;
    let folder = base.join("attachments");
    match fs::create_dir(&folder) {
        Ok(()) => crate::storage::sync_directory(base)?,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    crate::transfer::safe_directory(&folder)?;
    for (relative, bytes) in images {
        if *relative != reference(bytes) {
            return Err(io::Error::other("An image in this note is damaged."));
        }
        let path = base.join(relative);
        match crate::transfer::export_bytes_new(&path, bytes) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let metadata = path.symlink_metadata()?;
                if !metadata.file_type().is_file()
                    || metadata.len() != bytes.len() as u64
                    || crate::transfer::read_bounded(&path, MAX_BYTES)? != *bytes
                {
                    return Err(io::Error::other(
                        "An image in this note was changed by another app.",
                    ));
                }
            }
            Err(error) => return Err(error),
        }
    }
    crate::storage::sync_directory(&folder)
}

pub fn managed_source(text: &str, base: &Path) -> io::Result<(String, BTreeMap<String, Vec<u8>>)> {
    copy_source_images(text, base, true)
}
pub fn portable_source(text: &str, base: &Path) -> io::Result<(String, BTreeMap<String, Vec<u8>>)> {
    copy_source_images(text, base, false)
}
fn copy_source_images(
    text: &str,
    base: &Path,
    rebase_links: bool,
) -> io::Result<(String, BTreeMap<String, Vec<u8>>)> {
    let schema = commonmark_schema();
    let source = SourceDocument::parse(&schema, text).map_err(|e| {
        log::warn!("Cannot parse a note: {e}");
        io::Error::other("A note couldn’t be read as Markdown.")
    })?;
    crate::transfer::safe_directory(base)?;
    let base = base.canonicalize()?;
    let mut images = BTreeMap::new();
    let mut copied = BTreeMap::new();
    fn rewrite(
        node: &Node,
        base: &Path,
        schema: &markraft_core::Schema,
        images: &mut BTreeMap<String, Vec<u8>>,
        rebase_links: bool,
        copied: &mut BTreeMap<PathBuf, String>,
    ) -> io::Result<Node> {
        let mut result = node.copy(Fragment::from_nodes(
            node.children()
                .map(|n| rewrite(n, base, schema, images, rebase_links, copied))
                .collect::<io::Result<Vec<_>>>()?,
        ));
        let marks = node
            .marks()
            .iter()
            .map(|mark| {
                let mut mark = mark.clone();
                if let Some(href) = mark.attrs.get("href").and_then(|v| v.as_str())
                    && rebase_links
                    && !href.is_empty()
                    && !href.contains(':')
                    && !href.starts_with('#')
                    && !href.starts_with("//")
                {
                    let url = url::Url::from_directory_path(base)
                        .map_err(|_| {
                            io::Error::other("A link points to a folder Dahlia can’t use.")
                        })?
                        .join(href)
                        .map_err(io::Error::other)?;
                    mark.attrs = mark.attrs.with("href", url.to_string());
                }
                Ok(mark)
            })
            .collect::<io::Result<Vec<_>>>()?;
        result = result.mark(MarkSet::from_marks(schema, marks));
        if let Some(src) = node.attrs().get("src").and_then(|v| v.as_str()) {
            if !src.is_empty()
                && !src.contains(':')
                && !src.starts_with('#')
                && !src.starts_with("//")
            {
                let decoded = percent_encoding::percent_decode_str(src)
                    .decode_utf8()
                    .map_err(io::Error::other)?;
                let relative = Path::new(decoded.as_ref());
                if decoded.contains('\\')
                    || relative
                        .components()
                        .any(|part| !matches!(part, Component::Normal(_) | Component::CurDir))
                {
                    return Err(io::Error::other(
                        "An image is outside the note’s folder. Move it into the folder, then import again.",
                    ));
                }
                let mut path = base.to_owned();
                for part in relative.components() {
                    path.push(part);
                    if path
                        .symlink_metadata()
                        .map_err(|error| {
                            io::Error::other(format!(
                                "Image “{src}”: {}",
                                crate::copy::plain(&error)
                            ))
                        })?
                        .file_type()
                        .is_symlink()
                    {
                        return Err(io::Error::other(format!(
                            "Image “{src}” is an alias. Use the original file."
                        )));
                    }
                }
                let relative = if let Some(relative) = copied.get(&path) {
                    relative.clone()
                } else {
                    let relative = retain(images, read_image(&path)?)?;
                    copied.insert(path, relative.clone());
                    relative
                };
                result = result.with_attrs(node.attrs().with("src", relative));
            } else if src.starts_with("file:")
                || (src.len() >= 2
                    && src.as_bytes()[1] == b':'
                    && src.as_bytes()[0].is_ascii_alphabetic())
            {
                return Err(io::Error::other(
                    "An image is outside the note’s folder. Move it into the folder, then import again.",
                ));
            }
        }
        Ok(result)
    }
    let document = rewrite(
        source.document(),
        &base,
        &schema,
        &mut images,
        rebase_links,
        &mut copied,
    )?;
    let text = source.render(&schema, &document).map_err(|e| {
        log::warn!("Cannot render Markdown after copying images: {e}");
        io::Error::other("The images couldn’t be copied.")
    })?;
    Ok((text, images))
}

pub type PreparedImages = (Vec<String>, BTreeMap<String, Vec<u8>>);
pub fn clipboard_images(item: gpui::ClipboardItem) -> io::Result<PreparedImages> {
    let mut images = BTreeMap::new();
    let mut order = Vec::new();
    for entry in item.into_entries() {
        match entry {
            gpui::ClipboardEntry::Image(image) => {
                if order.len() >= MAX_IMAGES {
                    return Err(io::Error::other("Add up to 64 images (64 MB) at a time."));
                }
                order.push(retain(&mut images, normalize(image.bytes())?)?);
            }
            gpui::ClipboardEntry::ExternalPaths(paths) => {
                if order.len() + paths.0.len() > MAX_IMAGES {
                    return Err(io::Error::other("Add up to 64 images (64 MB) at a time."));
                }
                for path in &paths.0 {
                    order.push(retain(&mut images, read_image(path)?)?);
                }
            }
            _ => {}
        }
    }
    Ok((order, images))
}

pub fn export(path: &Path, text: &str, base: &Path) -> io::Result<()> {
    crate::transfer::new_destination(path)?;
    let (text, images) = portable_source(text, base)?;
    publish(
        path.parent()
            .ok_or_else(|| io::Error::other("Choose a folder."))?,
        &images,
    )?;
    crate::transfer::export_new(path, &text)
}

#[cfg(target_os = "macos")]
mod native_image {
    use super::*;
    use std::ffi::{CStr, CString, c_char, c_void};
    type Ref = *const c_void;
    struct Owned(Ref);
    impl Owned {
        fn new(value: Ref, operation: &str) -> io::Result<Self> {
            if value.is_null() {
                log::warn!("ImageIO could not {operation}.");
                Err(io::Error::other("Dahlia can’t read this image format."))
            } else {
                Ok(Self(value))
            }
        }
    }
    impl Drop for Owned {
        fn drop(&mut self) {
            // SAFETY: Owned stores one non-null reference returned by a CF Create/Copy call.
            unsafe {
                CFRelease(self.0);
            }
        }
    }
    // SAFETY: these declarations match CoreFoundation's C prototypes in CFBase.h, CFData.h,
    // CFDictionary.h, CFNumber.h and CFString.h; `Ref` stands for every CF object pointer.
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(value: Ref);
        fn CFGetTypeID(value: Ref) -> usize;
        fn CFNumberGetTypeID() -> usize;
        fn CFDataCreate(allocator: Ref, bytes: *const u8, length: isize) -> Ref;
        fn CFDataCreateMutable(allocator: Ref, capacity: isize) -> Ref;
        fn CFDataGetLength(data: Ref) -> isize;
        fn CFDataGetBytePtr(data: Ref) -> *const u8;
        fn CFDictionaryGetValue(dictionary: Ref, key: Ref) -> Ref;
        fn CFDictionaryCreate(
            allocator: Ref,
            keys: *const Ref,
            values: *const Ref,
            count: isize,
            key_callbacks: Ref,
            value_callbacks: Ref,
        ) -> Ref;
        fn CFNumberGetValue(number: Ref, kind: isize, result: *mut c_void) -> bool;
        fn CFNumberCreate(allocator: Ref, kind: isize, value: *const c_void) -> Ref;
        fn CFStringCreateWithCString(allocator: Ref, text: *const c_char, encoding: u32) -> Ref;
        fn CFStringGetCString(
            string: Ref,
            buffer: *mut c_char,
            capacity: isize,
            encoding: u32,
        ) -> bool;
        static kCFBooleanFalse: Ref;
        static kCFBooleanTrue: Ref;
    }
    // SAFETY: these declarations match ImageIO's CGImageSource.h, CGImageDestination.h and
    // CGImageProperties.h prototypes and constants.
    #[link(name = "ImageIO", kind = "framework")]
    unsafe extern "C" {
        fn CGImageSourceCreateWithData(data: Ref, options: Ref) -> Ref;
        fn CGImageSourceGetType(source: Ref) -> Ref;
        fn CGImageSourceCopyPropertiesAtIndex(source: Ref, index: usize, options: Ref) -> Ref;
        fn CGImageSourceCreateThumbnailAtIndex(source: Ref, index: usize, options: Ref) -> Ref;
        fn CGImageDestinationCreateWithData(
            data: Ref,
            kind: Ref,
            count: usize,
            options: Ref,
        ) -> Ref;
        fn CGImageDestinationAddImage(destination: Ref, image: Ref, properties: Ref);
        fn CGImageDestinationFinalize(destination: Ref) -> bool;
        static kCGImagePropertyPixelWidth: Ref;
        static kCGImagePropertyPixelHeight: Ref;
        static kCGImageSourceShouldCache: Ref;
        static kCGImageSourceCreateThumbnailFromImageAlways: Ref;
        static kCGImageSourceCreateThumbnailWithTransform: Ref;
        static kCGImageSourceThumbnailMaxPixelSize: Ref;
    }
    // SAFETY: these declarations match CoreGraphics' CGImage.h prototypes.
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGImageGetWidth(image: Ref) -> usize;
        fn CGImageGetHeight(image: Ref) -> usize;
    }
    /// A positive integer image property, such as the pixel width.
    ///
    /// # Safety
    ///
    /// `properties` must be a live CFDictionary for the duration of the call (as returned by
    /// `CGImageSourceCopyPropertiesAtIndex` and still owned by the caller), and `key` one of
    /// ImageIO's property-key constants.
    unsafe fn dimension(properties: Ref, key: Ref) -> io::Result<u64> {
        // SAFETY: caller retains the ImageIO properties dictionary and passes an ImageIO key.
        let value = unsafe { CFDictionaryGetValue(properties, key) };
        // SAFETY: a non-null dictionary value is a live CF object; this only inspects its type.
        let numeric = !value.is_null() && unsafe { CFGetTypeID(value) == CFNumberGetTypeID() };
        if !numeric {
            return Err(io::Error::other("This image is damaged."));
        }
        let mut number = 0i64;
        // SAFETY: type checked above; number points to a writable i64 matching kCFNumberSInt64Type.
        if !unsafe {
            CFNumberGetValue(
                value,
                4, /* kCFNumberSInt64Type */
                (&mut number as *mut i64).cast(),
            )
        } || number <= 0
        {
            return Err(io::Error::other("This image is damaged."));
        }
        Ok(number as u64)
    }
    pub(super) fn reencode(bytes: &[u8], output_type: &str) -> io::Result<Vec<u8>> {
        if bytes.len() > MAX_BYTES {
            return Err(io::Error::other("An image is larger than 16 MB. Choose a smaller one."));
        }
        // Header type and dimensions are checked before decoding frame zero.
        // SAFETY: Create/Copy results are retained by Owned and checked for null; all borrowed
        // values outlive their use. Input/output buffers remain live for each bounded CF call.
        unsafe {
            let null = std::ptr::null();
            let data = Owned::new(
                CFDataCreate(null, bytes.as_ptr(), bytes.len() as isize),
                "read image bytes",
            )?;
            let source = Owned::new(
                CGImageSourceCreateWithData(data.0, null),
                "read this image format",
            )?;
            let identifier = CGImageSourceGetType(source.0);
            let mut kind = [0 as c_char; 128];
            if identifier.is_null()
                || !CFStringGetCString(
                    identifier,
                    kind.as_mut_ptr(),
                    kind.len() as isize,
                    0x08000100,
                )
                || !matches!(
                    CStr::from_ptr(kind.as_ptr()).to_str().unwrap_or(""),
                    "public.png"
                        | "public.jpeg"
                        | "public.jpeg-2000"
                        | "public.tiff"
                        | "com.compuserve.gif"
                        | "com.microsoft.bmp"
                        | "org.webmproject.webp"
                        | "public.heic"
                        | "public.heif"
                        | "public.heif-standard"
                )
            {
                return Err(io::Error::other("Dahlia can’t read this image format."));
            }
            let properties = Owned::new(
                CGImageSourceCopyPropertiesAtIndex(source.0, 0, null),
                "read image dimensions",
            )?;
            let width = dimension(properties.0, kCGImagePropertyPixelWidth)?;
            let height = dimension(properties.0, kCGImagePropertyPixelHeight)?;
            validate_dimensions(width, height)?;
            let maximum = width.max(height) as i64;
            let number = Owned::new(
                CFNumberCreate(null, 4, (&maximum as *const i64).cast()),
                "set a decode bound",
            )?;
            let keys = [
                kCGImageSourceShouldCache,
                kCGImageSourceCreateThumbnailFromImageAlways,
                kCGImageSourceCreateThumbnailWithTransform,
                kCGImageSourceThumbnailMaxPixelSize,
            ];
            let values = [kCFBooleanFalse, kCFBooleanTrue, kCFBooleanTrue, number.0];
            let options = Owned::new(
                CFDictionaryCreate(
                    null,
                    keys.as_ptr(),
                    values.as_ptr(),
                    keys.len() as isize,
                    null,
                    null,
                ),
                "set decode options",
            )?;
            let image = Owned::new(
                CGImageSourceCreateThumbnailAtIndex(source.0, 0, options.0),
                "decode these image pixels",
            )?;
            validate_dimensions(
                CGImageGetWidth(image.0) as u64,
                CGImageGetHeight(image.0) as u64,
            )?;
            let output = Owned::new(CFDataCreateMutable(null, 0), "prepare PNG output")?;
            let output_type = CString::new(output_type).map_err(io::Error::other)?;
            let kind = Owned::new(
                CFStringCreateWithCString(null, output_type.as_ptr(), 0x08000100 /* UTF-8 */),
                "select an image encoder",
            )?;
            let destination = Owned::new(
                CGImageDestinationCreateWithData(output.0, kind.0, 1, null),
                "create the image encoder",
            )?;
            CGImageDestinationAddImage(destination.0, image.0, null);
            if !CGImageDestinationFinalize(destination.0) {
                return Err(io::Error::other("This image couldn’t be converted."));
            }
            let length = CFDataGetLength(output.0);
            if length <= 0 || length as usize > MAX_BYTES {
                return Err(io::Error::other(
                    "An image is too large after conversion (over 16 MB). Choose a smaller one.",
                ));
            }
            let pointer = CFDataGetBytePtr(output.0);
            if pointer.is_null() {
                return Err(io::Error::other("This image couldn’t be converted."));
            }
            Ok(std::slice::from_raw_parts(pointer, length as usize).to_vec())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn png(red: u8) -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(2, 2, image::Rgba([red, 0, 0, 255]));
        let mut bytes = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut bytes, ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }
    #[test]
    fn images_reuse_identity_and_portable_transfer_preserves_unrelated_source() {
        let _root_fixture = tempfile::tempdir().unwrap();
        let root = _root_fixture.path().to_path_buf();
        fs::create_dir_all(root.join("images")).unwrap();
        let mut png = Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(2, 2)
            .write_to(&mut png, ImageFormat::Png)
            .unwrap();
        fs::write(root.join("images/a.png"), png.get_ref()).unwrap();
        let source =
            "---\r\ntitle: kept\r\n---\r\n\r\n![image](images/a.png)\r\n\r\n<!-- kept -->\r\n";
        let (text, assets) = managed_source(source, &root).unwrap();
        assert_eq!(assets.len(), 1);
        assert!(text.starts_with("---\r\ntitle: kept\r\n---\r\n"));
        assert!(text.ends_with("<!-- kept -->\r\n"));
        publish(&root, &assets).unwrap();
        publish(&root, &assets).unwrap();
        assert_eq!(managed_source(&text, &root).unwrap().0, text);
        assert!(normalize(b"not an image").is_err());
        assert!(managed_source("![bad](../outside.png)", &root).is_err());
        let (linked, _) = managed_source("[other](other.md#section)\r\n", &root).unwrap();
        assert!(linked.contains("other.md#section"));
        assert!(
            !crate::transfer::source_has_linked_files("[remote](//example.com/a.md)\n").unwrap()
        );
        let source_with_link = "[other](other.md#section)\r\n\r\n<!-- kept -->\r\n";
        assert_eq!(
            portable_source(source_with_link, &root).unwrap().0,
            source_with_link
        );
    }

    #[test]
    fn clipboard_dedup_keeps_order_and_export_copies_portable_images() {
        let _root_fixture = tempfile::tempdir().unwrap();
        let root = _root_fixture.path().to_path_buf();
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("a.png"), png(1)).unwrap();
        fs::write(root.join("b.png"), png(2)).unwrap();
        let item = gpui::ClipboardItem {
            entries: vec![gpui::ClipboardEntry::ExternalPaths(gpui::ExternalPaths(
                vec![root.join("b.png"), root.join("a.png"), root.join("b.png")].into(),
            ))],
        };
        let (order, images) = clipboard_images(item).unwrap();
        assert_eq!(order.len(), 3);
        assert_eq!(images.len(), 2);
        assert_eq!(order[0], order[2]);
        assert_ne!(order[0], order[1]);
        publish(&root, &images).unwrap();
        let source = format!(
            "---\r\nkept: yes\r\n---\r\n\r\n![b]({})\r\n\r\n<!-- retained -->\r\n",
            order[0]
        );
        let output = root.join("portable");
        fs::create_dir(&output).unwrap();
        export(&output.join("note.md"), &source, &root).unwrap();
        assert_eq!(fs::read_to_string(output.join("note.md")).unwrap(), source);
        assert_eq!(fs::read(output.join(&order[0])).unwrap(), images[&order[0]]);
        assert!(export(&output.join("note.md"), "replacement", &root).is_err());
        assert_eq!(fs::read_to_string(output.join("note.md")).unwrap(), source);
    }

    #[test]
    fn validation_limits_and_attachment_collision_preserve_existing_bytes() {
        assert!(
            normalize(&vec![0; MAX_BYTES + 1])
                .unwrap_err()
                .to_string()
                .contains("16 MB")
        );
        let mut oversized = Vec::from(b"BM".as_slice());
        oversized.extend([0u8; 12]);
        oversized.extend(40u32.to_le_bytes());
        oversized.extend(100_000i32.to_le_bytes());
        oversized.extend(100_000i32.to_le_bytes());
        oversized.extend(1u16.to_le_bytes());
        oversized.extend(24u16.to_le_bytes());
        oversized.extend([0u8; 24]);
        assert!(normalize(&oversized).is_err());
        let _root_fixture = tempfile::tempdir().unwrap();
        let root = _root_fixture.path().to_path_buf();
        fs::create_dir_all(root.join("attachments")).unwrap();
        let bytes = normalize(&png(1)).unwrap();
        let relative = reference(&bytes);
        fs::write(root.join(&relative), b"existing").unwrap();
        assert!(publish(&root, &BTreeMap::from([(relative.clone(), bytes)])).is_err());
        assert_eq!(fs::read(root.join(&relative)).unwrap(), b"existing");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn native_imageio_rejects_icon_containers_before_pixel_decode() {
        let icon = include_bytes!("../assets/brand/dahlia.icns");
        assert!(native_image::reencode(icon, "public.png").is_err());
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn native_imageio_checks_png_and_heic_when_the_encoder_is_available() {
        let native_png = native_image::reencode(&png(20), "public.png").unwrap();
        assert_eq!(image::load_from_memory(&native_png).unwrap().width(), 2);
        assert!(native_image::reencode(b"malformed", "public.png").is_err());
        assert!(validate_dimensions(16_001, 1).is_err());
        assert!(validate_dimensions(8_000, 8_000).is_err());
        let mut encoded = Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(128, 128)
            .write_to(&mut encoded, ImageFormat::Png)
            .unwrap();
        let heic = match native_image::reencode(encoded.get_ref(), "public.heic") {
            Ok(bytes) => bytes,
            Err(error) => {
                eprintln!(
                    "HEIC fixture generation unavailable in this environment: {error}; native PNG and header bounds were checked."
                );
                return;
            }
        };
        assert!(normalize_rust(&heic).is_err());
        let normalized = normalize(&heic).unwrap();
        assert_eq!(image::load_from_memory(&normalized).unwrap().width(), 128);
        assert_eq!(image::load_from_memory(&normalized).unwrap().height(), 128);
        eprintln!(
            "Native HEIC fixture generated and normalized: {} -> {} bytes.",
            heic.len(),
            normalized.len()
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_and_traversal_cannot_import_or_publish_outside_the_note() {
        use std::os::unix::fs::symlink;
        let _root_fixture = tempfile::tempdir().unwrap();
        let root = _root_fixture.path().to_path_buf();
        fs::create_dir_all(root.join("note")).unwrap();
        fs::create_dir(root.join("outside")).unwrap();
        fs::write(root.join("outside/image.png"), png(1)).unwrap();
        symlink(root.join("outside"), root.join("note/images")).unwrap();
        assert!(
            managed_source("![bad](images/image.png)", &root.join("note"))
                .unwrap_err()
                .to_string()
                .contains("alias")
        );
        assert!(managed_source("![bad](%2e%2e/outside/image.png)", &root.join("note")).is_err());
        symlink(root.join("outside"), root.join("note/attachments")).unwrap();
        let bytes = normalize(&png(1)).unwrap();
        let relative = reference(&bytes);
        assert!(
            publish(
                &root.join("note"),
                &BTreeMap::from([(relative.clone(), bytes)])
            )
            .is_err()
        );
        assert!(
            !root
                .join("outside")
                .join(relative.trim_start_matches("attachments/"))
                .exists()
        );
        symlink(root.join("outside"), root.join("export-link")).unwrap();
        assert!(
            export(
                &root.join("export-link/note.md"),
                "source\n",
                &root.join("note")
            )
            .is_err()
        );
    }
}
