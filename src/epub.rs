// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with
// this file, You can obtain one at https://mozilla.org/MPL/2.0/.

use templates;
use errors::Result;
use errors::ResultExt;
use zip::Zip;
use toc::Toc;
use toc::TocElement;
use epub_content::EpubContent;
use epub_content::ReferenceType;

use std::io::Read;
use std::io::Write;
use std::path::Path;

use chrono;
use uuid;
use mustache::MapBuilder;

/// Represents the EPUB version.
///
/// Currently, this library supports EPUB 2.0.1 and 3.0.1.
#[derive(Debug, Copy, Clone, PartialOrd, PartialEq)]
pub enum EpubVersion {
    /// EPUB 2.0.1 format
    V20,
    /// EPUB 3.0.1 format
    V30,
    /// Hint that destructuring should not be exhaustive
    #[doc(hidden)]
    __NonExhaustive,
}

/// EPUB Metadata
#[derive(Debug)]
struct Metadata {
    pub title: String,
    pub author: String,
    pub lang: String,
    pub generator: String,
    pub toc_name: String,
    pub description: Option<String>,
    pub subject: Option<String>,
    pub license: Option<String>,
}

impl Metadata {
    /// Create new default metadata
    pub fn new() -> Metadata {
        Metadata {
            title: String::new(),
            author: String::new(),
            lang: String::from("en"),
            generator: String::from("Rust EPUB library"),
            toc_name: String::from("Table Of Contents"),
            description: None,
            subject: None,
            license: None,
        }
    }
}

/// A file added in the EPUB
#[derive(Debug)]
struct Content {
    pub file: String,
    pub mime: String,
    pub itemref: bool,
    pub cover: bool,
    pub reftype: Option<ReferenceType>,
    pub title: String,
}

impl Content {
    /// Create a new content file
    pub fn new<S1:Into<String>, S2: Into<String>>(file: S1, mime: S2) -> Content {
        Content {
            file: file.into(),
            mime: mime.into(),
            itemref: false,
            cover: false,
            reftype: None,
            title: String::new()
        }
    }
}

/// Epub Builder
///
/// The main struct you'll need to use in this library. It is first created using
/// a wrapper to zip files; then you add content to it, and finally you generate
/// the EPUB file by calling the `generate` method.
///
/// ```
/// use epub_builder::EpubBuilder;
/// use epub_builder::ZipCommand;
/// use std::io;
///
/// // "Empty" EPUB file
/// let mut builder = EpubBuilder::new(ZipCommand::new().unwrap()).unwrap();
/// builder.metadata("title", "Empty EPUB").unwrap();
/// builder.generate(&mut io::stdout()).unwrap();
/// ```
#[derive(Debug)]
pub struct EpubBuilder<Z:Zip> {
    version: EpubVersion,
    zip: Z,
    files: Vec<Content>,
    metadata: Metadata,
    toc: Toc,
    stylesheet: bool,
    inline_toc: bool,
}

impl<Z:Zip> EpubBuilder<Z> {
    /// Create a new default EPUB Builder
    pub fn new(zip: Z) -> Result<EpubBuilder<Z>> {
        let mut epub = EpubBuilder {
            version: EpubVersion::V20,
            zip: zip,
            files: vec!(),
            metadata: Metadata::new(),
            toc: Toc::new(),
            stylesheet: false,
            inline_toc: false,
        };
        
        // Write mimetype upfront
        epub.zip.write_file("mimetype", "application/epub+zip".as_bytes())?;
        epub.zip.write_file("META-INF/container.xml", templates::CONTAINER)?;
        epub.zip.write_file("META-INF/com.apple.ibooks.display-options.xml", templates::IBOOKS)?;

        Ok(epub)
    }

    /// Set EPUB version (default: V20)
    ///
    /// Supported versions are:
    ///
    /// * `V20`: EPUB 2.0.1
    /// * 'V30`: EPUB 3.0.1
    pub fn epub_version(&mut self, version: EpubVersion) -> &mut Self {
        self.version = version;
        self
    }

    /// Set some EPUB metadata
    ///
    /// # Valid keys used by the EPUB builder
    ///
    /// * `author`: author(s) of the book;
    /// * `title`: title of the book;
    /// * `lang`: the language ot the book, quite important as EPUB renderers rely on it
    ///   for e.g. hyphenating words.
    /// * `generator`: generator of the book (should be your program name);
    /// * `toc_name`: the name to use for table of contents (by default, "Table of Contents");
    /// * `subject`;
    /// * `description`;
    /// * `license`.

    pub fn metadata<S1: AsRef<str>, S2: Into<String>>(&mut self, key: S1, value: S2) -> Result<&mut Self> {
        match key.as_ref() {
            "author" => self.metadata.author = value.into(),
            "title" => self.metadata.title = value.into(),
            "lang" => self.metadata.lang = value.into(),
            "generator" => self.metadata.generator = value.into(),
            "description" => self.metadata.description = Some(value.into()),
            "subject" => self.metadata.subject = Some(value.into()),
            "license" => self.metadata.license = Some(value.into()),
            "toc_name" => self.metadata.toc_name = value.into(),
            s => bail!("invalid metadata '{}'", s),
        }
        Ok(self)
    }

    /// Sets stylesheet of the EPUB.
    ///
    /// This content will be written in a `stylesheet.css` file; it is used by
    /// some pages (such as nav.xhtml), you don't have use it in your documents though it
    /// makes sense to also do so.
    pub fn stylesheet<R:Read>(&mut self, content: R) -> Result<&mut Self> {
        self.add_resource("stylesheet.css", content, "text/css")?;
        self.stylesheet = true;
        Ok(self)
    }

    /// Adds an inline toc in the document.
    ///
    /// If this method is called it adds a page that contains the table of contents
    /// that appears in the document.
    ///
    /// The position where this table of contents will be inserted depends on when
    /// you call this method: if you call it before adding any content, it will be
    /// at the beginning, if you call it after, it will be at the end.
    pub fn inline_toc(&mut self) -> &mut Self {
        self.inline_toc = true;
        self.toc.add(TocElement::new("toc.xhtml", self.metadata.toc_name.as_ref()));
        let mut file = Content::new("toc.xhtml", "application/xhtml+xml");
        file.reftype = Some(ReferenceType::Toc);
        file.title = self.metadata.toc_name.clone();
        file.itemref = true;
        self.files.push(file);
        self
    }
    

    /// Add a resource to the EPUB file
    ///
    /// This resource be a picture, a font, some CSS file, .... Unlike
    /// `add_content`, files added this way won't appear in the linear
    /// document.
    ///
    /// Note that these files will automatically be inserted into an `OEBPS` directory,
    /// so you don't need (and shouldn't) prefix your path with `OEBPS/`.
    ///
    /// # Arguments
    ///
    /// * `path`: the path where this file will be writen in the EPUB OEBPS structure,
    ///   e.g. `data/image_0.png`
    /// * `content`: the resource to include
    /// * `mime_type`: the mime type of this file, e.g. "image/png".
    pub fn add_resource<R: Read, P: AsRef<Path>, S: Into<String>>(&mut self,
                                                                  path: P,
                                                                  content: R,
                                                                  mime_type: S) -> Result<&mut Self> {
        self.zip.write_file(Path::new("OEBPS").join(path.as_ref()), content)?;
        self.files.push(Content::new(format!("{}", path.as_ref().display()), mime_type));
        Ok(self)
    }

    /// Add a cover image to the EPUB.
    ///
    /// This works similarly to adding the image as a resource with the `add_resource`
    /// method, except, it signals it in the Manifest secton so it is displayed as the
    /// cover by Ereaders
    pub fn add_cover_image<R: Read, P: AsRef<Path>, S: Into<String>>(&mut self,
                                                                     path: P,
                                                                     content: R,
                                                                     mime_type: S) -> Result<&mut Self> {
        self.zip.write_file(Path::new("OEBPS").join(path.as_ref()), content)?;
        let mut file = Content::new(format!("{}", path.as_ref().display()), mime_type);
        file.cover = true;
        self.files.push(file);
        Ok(self)
    }
    
    /// Add a XHTML content file that will be added to the EPUB.
    ///
    /// # Examples
    ///
    /// ```
    /// # use epub_builder::{EpubBuilder, ZipLibrary, EpubContent};
    /// let content = "Some content";
    /// let mut builder = EpubBuilder::new(ZipLibrary::new()).unwrap();
    /// // Add a chapter that won't be added to the Table of Contents 
    /// builder.add_content(EpubContent::new("intro.xhtml", content.as_bytes())).unwrap();
    /// ```
    ///
    /// ```
    /// # use epub_builder::{EpubBuilder, ZipLibrary, EpubContent, TocElement};
    /// # let mut builder = EpubBuilder::new(ZipLibrary::new()).unwrap();
    /// # let content = "Some content";
    /// // Sets the title of a chapter so it is added to the Table of contents
    /// // Also add information about its structure
    /// builder.add_content(EpubContent::new("chapter_1.xhtml", content.as_bytes())
    ///                      .title("Chapter 1")
    ///                      .child(TocElement::new("chapter_1.xhtml#1", "1.1"))).unwrap();
    /// ```
    ///
    /// ```
    /// # use epub_builder::{EpubBuilder, ZipLibrary, EpubContent};
    /// # let mut builder = EpubBuilder::new(ZipLibrary::new()).unwrap();
    /// # let content = "Some content";
    /// // Add a section, by setting the level to 2 (instead of the default value 1)
    /// builder.add_content(EpubContent::new("section.xhtml", content.as_bytes())
    ///                      .title("Section 1")
    ///                      .level(2)).unwrap();
    /// ```
    ///
    /// Note that these files will automatically be inserted into an `OEBPS` directory,
    /// so you don't need (and shouldn't) prefix your path with `OEBPS/`.
    ///
    /// # See also
    ///
    /// * [`EpubContent`](struct.EpubContent.html)
    /// * the `add_resource` method, to add other resources in the EPUB file.
    pub fn add_content<R: Read>(&mut self, content: EpubContent<R>)-> Result<&mut Self> {
        self.zip.write_file(Path::new("OEBPS").join(content.toc.url.as_str()),
                            content.content)?;
        let mut file = Content::new(content.toc.url.as_ref(),
                                "application/xhtml+xml");
        file.itemref = true;
        file.reftype = content.reftype;
        if file.reftype.is_some() {
            file.title = content.toc.title.clone();
        }
        self.files.push(file);
        if !content.toc.title.is_empty() {
            self.toc.add(content.toc);
        }
        Ok(self)
    }

    /// Generate the EPUB file and write it to the writer
    ///
    /// # Example
    ///
    /// ```
    /// # use epub_builder::{EpubBuilder, ZipLibrary};
    /// let mut builder = EpubBuilder::new(ZipLibrary::new()).unwrap();
    /// // Write the EPUB file into a Vec<u8>
    /// let mut epub: Vec<u8> = vec!();
    /// builder.generate(&mut epub).unwrap();
    /// ```
    pub fn generate<W: Write>(&mut self, to: W) -> Result<()> {
        // If no styleesheet was provided, generate a dummy one
        if !self.stylesheet {
            self.stylesheet("".as_bytes())?;
        }
        /// Render content.opf
        let bytes = self.render_opf()?;
        self.zip.write_file("OEBPS/content.opf", &bytes as &[u8])?;
        /// Render toc.ncx
        let bytes = self.render_toc()?;
        self.zip.write_file("OEBPS/toc.ncx", &bytes as &[u8])?;
        // Render nav.xhtml
        let bytes = self.render_nav(true)?;
        self.zip.write_file("OEBPS/nav.xhtml", &bytes as &[u8])?;
        // Write inline toc if it needs to
        if self.inline_toc {
            let bytes = self.render_nav(false)?;
            self.zip.write_file("OEBPS/toc.xhtml", &bytes as &[u8])?;
        }

        
        self.zip.generate(to)?;
        Ok(())
    }

    /// Render content.opf file
    fn render_opf(&mut self) -> Result<Vec<u8>> {
        let mut optional = String::new();
        if let Some(ref desc) = self.metadata.description {
            optional.push_str(&format!("<dc:description>{}</dc:description>\n", desc));
        }
        if let Some(ref subject) = self.metadata.subject {
            optional.push_str(&format!("<dc:subject>{}</dc:subject>\n", subject));
        }
        if let Some(ref rights) = self.metadata.license {
            optional.push_str(&format!("<dc:rights>{}</dc:rights>\n", rights));
        }
        let date = chrono::UTC::now().format("%Y-%m-%dT%H:%M:%SZ");
        let uuid = uuid::Uuid::new_v4().urn().to_string();

        let mut items = String::new();
        let mut itemrefs = String::new();
        let mut guide = String::new();

        for content in self.files.iter() {
            let id = if content.cover { String::from("cover-image") } else { to_id(&content.file) };
            let properties = match (self.version, content.cover) {
                (EpubVersion::V30, true) => "cover-image",
                _ => "",
            };
            if content.cover {
                optional.push_str(&format!("<meta name = \"cover\" content = \"{}\" />\n",
                                           content.file));
            }
            items.push_str(&format!("<item media-type = \"{mime}\" \
                                     {properties} \
                                     id = \"{id}\" \
                                     href = \"{href}\" />\n",
                                    properties = properties,
                                    mime = content.mime,
                                    id = id,
                                    href = content.file));
            if content.itemref {
                itemrefs.push_str(&format!("<itemref idref = \"{id}\" />\n",
                                           id = id));
                                            
            }
            if let Some(reftype) = content.reftype {
                use epub_content::ReferenceType::*;
                let reftype = match reftype {
                    Cover => "cover",
                    TitlePage => "title-page",
                    Toc => "toc",
                    Index => "index",
                    Glossary => "glossary",
                    Acknowledgements => "acknowledgements",
                    Bibliography => "bibliography",
                    Colophon => "colophon",
                    Copyright => "copyright",
                    Dedication => "dedication",
                    Epigraph => "epigraph",
                    Foreword => "foreword",
                    Loi => "loi",
                    Lot => "lot",
                    Notes => "notes",
                    Preface => "preface",
                    Text => "text",
                };
                guide.push_str(&format!("<reference type = \"{reftype}\" \
                                         title = \"{title}\" \
                                         href = \"{href}\" />\n",
                                        reftype = reftype,
                                        title = content.title,
                                        href = content.file));
            }
        }

        let data = MapBuilder::new()
            .insert_str("lang", &self.metadata.lang)
            .insert_str("author", &self.metadata.author)
            .insert_str("title", &self.metadata.title)
            .insert_str("generator", &self.metadata.generator)
            .insert_str("toc_name", &self.metadata.toc_name)
            .insert_str("optional", optional)
            .insert_str("items", items)
            .insert_str("itemrefs", itemrefs)
            .insert_str("date", date)
            .insert_str("uuid", uuid)
            .insert_str("guide", guide)
            .build();

        let mut content = vec!();
        let res = match self.version {
            EpubVersion::V20 => templates::v2::CONTENT_OPF.render_data(&mut content, &data),
            EpubVersion::V30 => templates::v3::CONTENT_OPF.render_data(&mut content, &data),
            EpubVersion::__NonExhaustive => unreachable!(),
        };
        

        res
            .chain_err(|| "could not render template for content.opf")?;

        Ok(content)
    }

        /// Render toc.ncx
    fn render_toc(&mut self) -> Result<Vec<u8>> {
        let mut nav_points = String::new();

        nav_points.push_str(&self.toc.render_epub());

        let data = MapBuilder::new()
            .insert_str("toc_name", &self.metadata.toc_name)
            .insert_str("nav_points", nav_points)
            .build();
        let mut res: Vec<u8> = vec![];
        templates::TOC_NCX.render_data(&mut res, &data)
            .chain_err(|| "error rendering toc.ncx template")?;
        Ok(res)
    }


    /// Render nav.xhtml
    fn render_nav(&mut self, numbered: bool) -> Result<Vec<u8>> {
        let content = self.toc.render(numbered);
        let mut landmarks = String::new();
        if self.version > EpubVersion::V20 {
            for file in self.files.iter() {
                if let Some(ref reftype) = file.reftype {
                    use ReferenceType::*;
                    let reftype = match *reftype {
                        Cover => "cover",
                        Text => "bodymatter",
                        Toc => "toc",
                        Bibliography => "bibliography",
                        Epigraph => "epigraph",
                        Foreword => "foreword",
                        Preface => "preface",
                        Notes => "endnotes",
                        Loi => "loi",
                        Lot => "lot",
                        Colophon => "colophon",
                        TitlePage => "titlepage",
                        Index => "index",
                        Glossary => "glossary",
                        Copyright => "copyright-page",
                        Acknowledgements => "acknowledgements",
                        Dedication => "dedication"
                    };
                    landmarks.push_str(&format!("<li><a epub:type=\"{reftype}\" \
                                                 href = \"{href}\">\
                                                 {title}</a></li>\n",
                                                reftype = reftype,
                                                href = file.file,
                                                title = file.title));
                }
            }
        }
        if !landmarks.is_empty() {
            landmarks = format!("<ol>\n{}\n</ol>", landmarks);
        }
        
        let data = MapBuilder::new()
            .insert_str("content", content)
            .insert_str("toc_name", &self.metadata.toc_name)
            .insert_str("generator", &self.metadata.generator)
            .insert_str("landmarks", landmarks)
            .build();

        let mut res = vec!();
        let eh = match self.version {
            EpubVersion::V20 => templates::v2::NAV_XHTML.render_data(&mut res, &data),
            EpubVersion::V30 => templates::v3::NAV_XHTML.render_data(&mut res, &data),
            EpubVersion::__NonExhaustive => unreachable!(),
        };
        
        eh.chain_err(|| "error rendering nav.xhtml template")?;
        Ok(res)
    }
}


// generate an id compatible string, replacing / and . by _
fn to_id(s: &str) -> String {
    s.replace(".", "_").replace("/", "_")
}
