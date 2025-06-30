// Import necessary crates and modules.
// parking_lot::Mutex is used for efficient and ergonomic thraed-safe mutable access.
use parking_lot::Mutex;

// ropey::Rope is a performant text buffer data structure, suitable for editors.
use ropey::Rope;

// Arc provides shared ownership, essential for multiple components accessing the same textBuffer.
use std::sync::Arc;

// For defining asynchronous methods in traits.
use async_trait::async_trait;

// Tokio's mpsc channel for asynchronous message passing, a common Rust idiom for the Observer pattern.
use tokio::sync::mpsc;


// Text Position Struct
// Represents a specific position within the text buffer, typically a byte index.
// This is a core data model entity.
#[derive(Debug, Clone, Clone, PartialEq, Eq)]
pub struct TextPosition {
    pub byte_idx: usize,
}


// Text Buffer Change Event
// This enum defines the types of events that the TextBuffer can emit.
// It's part of the Observer pattern, carrying data about the change.
#[derive(Debug, Clone)]
pub enum TextBufferChangedEvent {
    // Event indicating that a range of text has been inserted.
    // Contains the starting byte index and the length of the inserted text.
    Inserted {
        start_byte_idx: usize,
        len_bytes: usize,
    },

    // Event indicating that a range of text has been removed.
    // Contains the starting byte index and the length of the removed text.
    Removed {
        start_byte_idx: usize,
        len_bytes: usize,
    },
}


// ITextBufferObserver Trait (Observer Pattern)
// Defines the contract for any component that wants to "observe" changes in a TextBuffer.
// This trait adheres to the Interface Segregation Principle (ISP) as it's specific to buffer changes.
// It uses 'async_trait' because UI updates or other reactions might involve asynchronous operations.
#[async_trait]
pub trait ITextBufferObserver: send + Sync {
    // This method is called when the TextBuffer content changes.
    async fn on_buffer_changed(&self, event: TextBufferChangedEvent);
}


// TextBuffer Entity
// Manage the actual text content of a document.
// Adheres to the Single Responsibility Principle (SRP) by solely focusing on text storage and manipulation.
// Uses 'Arc<Mutex<Rope>>' for thread-safe shared ownership, allowing multiple parts of the application
// (e.g., UI, LSP client) to access and modify the same buffer safely.
pub struct TextBuffer {
    // The core text data structure. 'Mutex' for module access, 'Arc' for shared ownership.
    content: Arc<Mutex<Rope>>,

    // Sender part of an MPSC channel to send change events to registered observers.
    // 'Vec' of senders allows multiple observers to receive messages.
    observers: Arc<Mutex<Vec<mpsc::Sender<TextBufferChangedEvent>>>>,
}


impl TextBuffer {
    // Create a new 'TextBuffer' instance with initial text.
    pub fn new(initial_text:  &str) -> Self {
        Self {
            content: Arc::new(Mutex::new(Rope::from_str(initial_text))),
            observers: Arc::new(Mutex::new(Vec::new())),
        }
    }


    // Public API for TextBuffer Manipulation

    // Inserts text at a given byte position.
    pub async fn insert(&self, position: TextPosition, text: &str) {
        let mut rope = self.content.lock(); // Acquire lock for mutable access
        rope.insert(position.byte_idx, text); // Perform the insertion
        drop(rope); // Release lock as soon as mutable operation is done

        // Notify observers asynchronously
        self.notify_observers(TextBufferChangedEvent::Inserted {
            start_byte_idx: position.byte_idx,
            len_bytes: text.len(),
        }).await;
    }


    // Removes text from a given byte position for a specified length.
    pub async fn remove(&self, position: TextPosition, len_bytes: usize) {
        let mut rope = self.content.lock(); // Acquire lock
        rope.remove(position.byte_idx..(position.byte_idx + len_bytes)); // Perform removal
        drop(rope);

        // Notify observers asynchronously
        self.notify_observers(TextBufferChangedEvent::Removed {
            start_byte_idx: position.byte_idx,
            len_bytes,
        }).await;
    }


    // Retrieves the entire text content as a String.
    pub fn get_text(&self) -> String {
        self.content.lock().to_string() // Acquire lock, convert Rope to String
    }


    // Retrieves a substring within the given byte range.
    pub fn get_range(&self, start_byte_idx: usize, end_byte_idx: usize) -> String {
        self.content.lock()
            .slice(start_byte_idx..end_byte_idx)
            .to_string()
    }


    // Returns the total length of the text in bytes.
    pub fn len_bytes(&self) -> usize {
        self.content.lock().len_bytes()
    }


    // Returns the total number of lines.
    pub fn len_lines(&self) -> usize {
        self.content.lock().len_lines()
    }

    // Observer Management

    // Adds a new observer. The observer should provide a channel sender.
    // This allows decoupling the observer's implementation from the TextBuffer.
    pub fn add_observer(&self, sender: mpsc::Sender<TextBufferChangedEvent>) {
        self.observers.lock().push(sender);
    }

    // Internal helper to notify all registered observers.
    async fn notify_observers(&self, event: TextBufferChangedEvent) {
        let observers = self.observers.lock();
        // Iterate through senders and attempt to send the event.
        // Remove disconnected channels to clean up.
        let mut disconnected_senders = Vec::new();
        for (i, sender) in observers.iter().enumerate() {
            if sender.send(event.clone()).await.is_err() {
                // If send fails, it means the receiver part of the channel is dropped.
                // mark this sender for removal.
                disconnected_senders.push(i);
            }
        }

        // Remove disconnected senders in reverse order to avoid index shifting issues.
        let mut observers_mut = observers.into_mut(); // Gain mutable access to Vec inside the Mutex
        for &idx in disconnected_senders.iter().rev() {
            observers_mut.remove(idx);
        }
    }

}

// Document Entity
// Represent a single file or document open in the IDE.
// Adheres to SRP by managing document-level properties, not the raw text content (which is textBuffer's job).
pub struct Document {
    pub file_path: Option<String>, // Path to file, None for unsaved new documents
    pub text_buffer: Arc<TextBuffer>, // Shared reference to the associated text buffer
    is_dirty: Mutex<bool>, // Indicated if the document has unsaved changes
    pub language_id: String, // e.g., "rust", "cpp", "plaintext"
}

impl Document {
    // Creates a new Document, optionally from an existing file path and initial content.
    pub fn new(file_path: Option<String>, initial_content: &str, language_id: String) -> Self {
        Self {
            file_path,
            text_buffer: Arc::new(TextBuffer::new(initial_content)),
            is_dirty: Mutex::new(false), // New documents are initially clean until modified
            language_id,
        }
    }

    // Returns true if the document has unsaved changes.
    pub fn is_dirty(&self) -> bool {
        *self.is_dirty.lock()
    }

    // Sets the dirty state of the document.
    pub fn set_dirty(&self, dirty: bool) {
        *self.is_dirty.lock() = dirty;
    }

    // Returns a reference to the underlying textBuffer.
    pub fn get_text_buffer(&self) -> Arc<TextBuffer> {
        Arc::clone(&self.text_buffer)
    }

    // Helper to get the filename from the path, or "Untitled" for new docs.
    pub fn file_name(&self) -> String {
        self.file_path.as_ref()
            .and_then(|p| std::Path::new(p).file_name())
            .and_then(|os_str| os_str.to_str())
            .map_or_else(|| "Untitled".to_string(), |s| s.to_string())
    }
}



