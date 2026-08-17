use std::path::PathBuf;

use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime, Duration};
use typst::syntax::{FileId, Source};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};
use typst_layout::PagedDocument;
use typst_pdf::PdfOptions;

struct HarnessWorld {
    library: LazyHash<Library>,
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
    source: Source,
}

impl HarnessWorld {
    fn new(source: String, font_paths: &[PathBuf]) -> Result<Self, String> {
        let mut fonts = Vec::new();
        for path in font_paths {
            let data = std::fs::read(path)
                .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
            fonts.extend(Font::iter(Bytes::new(data)));
        }
        if fonts.is_empty() {
            return Err("no usable fonts were supplied".into());
        }

        Ok(Self {
            library: LazyHash::new(Library::default()),
            book: LazyHash::new(FontBook::from_fonts(&fonts)),
            fonts,
            source: Source::detached(source),
        })
    }
}

impl World for HarnessWorld {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &self.book
    }

    fn main(&self) -> FileId {
        self.source.id()
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.source.id() {
            Ok(self.source.clone())
        } else {
            Err(FileError::NotFound(id.vpath().get_without_slash().into()))
        }
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        Err(FileError::NotFound(id.vpath().get_without_slash().into()))
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.get(index).cloned()
    }

    fn today(&self, _: Option<Duration>) -> Option<Datetime> {
        None
    }
}

fn main() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1).map(PathBuf::from);
    let source_path = args
        .next()
        .ok_or("usage: unicode-pdf-harness SOURCE.typ OUTPUT.pdf FONT...")?;
    let output_path = args
        .next()
        .ok_or("usage: unicode-pdf-harness SOURCE.typ OUTPUT.pdf FONT...")?;
    let font_paths: Vec<_> = args.collect();
    if font_paths.is_empty() {
        return Err("at least one font path is required".into());
    }

    let source = std::fs::read_to_string(&source_path)
        .map_err(|error| format!("failed to read {}: {error}", source_path.display()))?;
    let world = HarnessWorld::new(source, &font_paths)?;
    let compiled = typst::compile::<PagedDocument>(&world);
    for warning in &compiled.warnings {
        eprintln!("warning: {warning:?}");
    }
    let document = compiled
        .output
        .map_err(|errors| format!("Typst compilation failed: {errors:#?}"))?;
    let pdf = typst_pdf::pdf(&document, &PdfOptions::default())
        .map_err(|errors| format!("PDF export failed: {errors:#?}"))?;
    std::fs::write(&output_path, pdf)
        .map_err(|error| format!("failed to write {}: {error}", output_path.display()))?;
    println!("{}", output_path.display());
    Ok(())
}
