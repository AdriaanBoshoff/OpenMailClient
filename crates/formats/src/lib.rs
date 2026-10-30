//! Message body formats, read into one formatted document ([`rich::Doc`]) that the reading pane
//! draws. Reading is inert: nothing in a body is run, fetched or followed.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod canvas;
mod emf;
pub mod html;
pub mod ics;
pub mod links;
pub mod metafile;
pub mod rich;
pub mod rtf;
pub mod vcf;
pub mod wav;
mod wmf;
mod words;

#[cfg(test)]
mod tests;
