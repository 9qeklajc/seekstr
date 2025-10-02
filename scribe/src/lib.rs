pub mod backends;
pub mod processor;

// Re-export commonly used types
pub use backends::{create_backend, create_backend_auto};
pub use processor::{
    FileType, ProcessedContent, ProcessingResult, Processor,
    get_file_type_from_url, process_single_url_direct
};