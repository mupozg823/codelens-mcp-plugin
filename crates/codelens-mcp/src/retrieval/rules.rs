//! Rule retrieval — corpus loader + BM25F lexical search.
//!
//! CLAUDE.md global + project-local rules and the project's auto-memory
//! files are indexed here so `analyze_change_request` can eventually
//! inject a "relevant_rules" section without polluting the code
//! embedding index. Keeping the two corpora isolated is a deliberate
//! design choice: a semantic query for "database mock helper" should
//! return code chunks, not the CLAUDE.md rule "don't mock the database".

mod corpus;
mod scoring;

#[allow(unused_imports)]
pub use corpus::{RuleSnippet, RuleSource, load_rule_corpus, project_slug};
#[allow(unused_imports)]
pub use scoring::{ScoredSnippet, find_relevant_rules};
