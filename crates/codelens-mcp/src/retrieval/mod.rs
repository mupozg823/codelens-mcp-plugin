//! BM25 sparse retrieval lanes — agent rules + code symbols.
//!
//! Two sibling sparse retrieval surfaces, kept separate because their
//! corpus-building heuristics differ (markdown chunks vs symbol-level
//! documents) but paired under one namespace so callers can see them
//! as the "lexical side" of CodeLens retrieval.

pub(crate) mod rules;
pub(crate) mod symbols;
