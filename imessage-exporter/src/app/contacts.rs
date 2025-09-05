/*!
 VCF (vCard) contact parser for resolving contact names from phone numbers and email addresses.
*/

use std::{
    collections::HashMap,
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
};

use crate::app::error::RuntimeError;

/// Represents a contact parsed from a VCF file
#[derive(Debug, Clone)]
pub struct Contact {
    /// The full name of the contact
    pub name: String,
    /// Phone numbers associated with this contact
    pub phone_numbers: Vec<String>,
    /// Email addresses associated with this contact
    pub emails: Vec<String>,
}

/// VCF parser that extracts contact information and creates lookup maps
pub struct VcfParser {
    /// Map from phone number to contact name
    pub phone_to_name: HashMap<String, String>,
    /// Map from email address to contact name
    pub email_to_name: HashMap<String, String>,
    /// Temporary storage for contacts during parsing (for merging duplicates)
    contacts_by_name: HashMap<String, Contact>,
}

impl VcfParser {
    /// Create a new VCF parser
    pub fn new() -> Self {
        Self {
            phone_to_name: HashMap::new(),
            email_to_name: HashMap::new(),
            contacts_by_name: HashMap::new(),
        }
    }

    /// Parse a VCF file and populate the lookup maps
    pub fn parse_vcf_file<P: AsRef<Path>>(&mut self, vcf_path: P) -> Result<(), RuntimeError> {
        let file = File::open(vcf_path)?;
        let reader = BufReader::new(file);
        
        let mut current_contact = Contact {
            name: String::new(),
            phone_numbers: Vec::new(),
            emails: Vec::new(),
        };
        let mut in_vcard = false;

        for line in reader.lines() {
            let line = line?;
            let line = line.trim();

            if line == "BEGIN:VCARD" {
                in_vcard = true;
                current_contact = Contact {
                    name: String::new(),
                    phone_numbers: Vec::new(),
                    emails: Vec::new(),
                };
            } else if line == "END:VCARD" && in_vcard {
                // Store the contact for potential merging
                self.store_contact_for_merging(current_contact.clone());
                in_vcard = false;
            } else if in_vcard {
                self.parse_vcard_line(line, &mut current_contact);
            }
        }

        // After parsing all contacts, finalize by processing merged contacts
        self.finalize_contacts();

        Ok(())
    }

    /// Parse a single line from a vCard entry
    fn parse_vcard_line(&self, line: &str, contact: &mut Contact) {
        if let Some((field, value)) = line.split_once(':') {
            match field {
                "FN" => {
                    // Full name field
                    if !value.is_empty() {
                        contact.name = value.to_string();
                    }
                }
                field if field.starts_with("TEL") => {
                    // Phone number field
                    if !value.is_empty() {
                        contact.phone_numbers.push(self.normalize_phone_number(value));
                    }
                }
                field if field.starts_with("EMAIL") || field.contains(".EMAIL") => {
                    // Email field (handles both "EMAIL" and "item1.EMAIL" formats)
                    if !value.is_empty() {
                        contact.emails.push(value.trim().to_lowercase());
                    }
                }
                _ => {
                    // Ignore other fields for now
                }
            }
        }
    }

    /// Store a contact for potential merging with duplicates
    fn store_contact_for_merging(&mut self, contact: Contact) {
        // Only process contacts that have a proper name (FN field)
        // Skip contacts without names to avoid incorrect associations
        if contact.name.is_empty() {
            return;
        }

        let name = contact.name.clone();
        
        // Check if we already have a contact with this name
        if let Some(existing_contact) = self.contacts_by_name.get_mut(&name) {
            // Merge phone numbers (avoid duplicates)
            for phone in contact.phone_numbers {
                if !existing_contact.phone_numbers.contains(&phone) {
                    existing_contact.phone_numbers.push(phone);
                }
            }
            
            // Merge email addresses (avoid duplicates)
            for email in contact.emails {
                if !existing_contact.emails.contains(&email) {
                    existing_contact.emails.push(email);
                }
            }
        } else {
            // First time seeing this contact name
            self.contacts_by_name.insert(name, contact);
        }
    }

    /// Finalize contacts by processing all merged contacts into lookup maps
    fn finalize_contacts(&mut self) {
        // Collect contacts to avoid borrow checker issues
        let contacts: Vec<Contact> = self.contacts_by_name.values().cloned().collect();
        
        // Clear the temporary storage first
        self.contacts_by_name.clear();
        
        // Now process all contacts
        for contact in contacts {
            self.process_contact(contact);
        }
    }

    /// Process a completed contact and add it to the lookup maps
    fn process_contact(&mut self, contact: Contact) {
        // Add phone number mappings
        for phone in &contact.phone_numbers {
            self.phone_to_name.insert(phone.clone(), contact.name.clone());
        }

        // Add email mappings
        for email in &contact.emails {
            self.email_to_name.insert(email.clone(), contact.name.clone());
        }
    }

    /// Normalize a phone number by removing formatting characters
    fn normalize_phone_number(&self, phone: &str) -> String {
        // Remove common formatting characters but keep the core number
        let mut normalized = phone
            .chars()
            .filter(|c| c.is_ascii_digit() || *c == '+')
            .collect::<String>();
        
        // Remove leading country code if present (e.g., +1 for North America)
        if normalized.starts_with("+1") && normalized.len() == 12 {
            normalized = normalized[2..].to_string();
        } else if normalized.starts_with("1") && normalized.len() == 11 {
            normalized = normalized[1..].to_string();
        }
        
        normalized
    }

    /// Look up a contact name by phone number
    pub fn get_name_by_phone(&self, phone: &str) -> Option<&String> {
        let normalized = self.normalize_phone_number(phone);
        
        // Try exact match first
        if let Some(name) = self.phone_to_name.get(&normalized) {
            return Some(name);
        }

        // Try fuzzy matching - check if any stored number contains or is contained in the query
        for (stored_phone, name) in &self.phone_to_name {
            if self.phones_match(&normalized, stored_phone) {
                return Some(name);
            }
        }

        None
    }

    /// Look up a contact name by email address
    pub fn get_name_by_email(&self, email: &str) -> Option<&String> {
        self.email_to_name.get(&email.to_lowercase())
    }

    /// Check if two phone numbers match, accounting for different formatting
    fn phones_match(&self, phone1: &str, phone2: &str) -> bool {
        // Use normalized phone numbers for comparison
        let norm1 = self.normalize_phone_number(phone1);
        let norm2 = self.normalize_phone_number(phone2);
        
        // Exact match first
        if norm1 == norm2 {
            return true;
        }
        
        // Only do fuzzy matching if both numbers are reasonable phone number lengths
        if norm1.len() < 7 || norm2.len() < 7 {
            return false;
        }
        
        // Check if one ends with the other (for cases with different country code handling)
        // But only if the difference is reasonable (country code length)
        if norm1.len() >= 10 && norm2.len() >= 10 {
            let len_diff = (norm1.len() as i32 - norm2.len() as i32).abs();
            if len_diff <= 3 {  // Allow for country code differences
                return norm1.ends_with(&norm2) || norm2.ends_with(&norm1);
            }
        }
        
        false
    }


    /// Get the total number of contacts loaded
    pub fn contact_count(&self) -> usize {
        let phone_contacts = self.phone_to_name.len();
        let email_contacts = self.email_to_name.len();
        // Note: This might double-count contacts that have both phone and email
        phone_contacts + email_contacts
    }
}

impl Default for VcfParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_parse_simple_vcard() {
        let mut parser = VcfParser::new();
        
        // Create a temporary VCF file
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "BEGIN:VCARD").unwrap();
        writeln!(temp_file, "VERSION:3.0").unwrap();
        writeln!(temp_file, "FN:John Doe").unwrap();
        writeln!(temp_file, "TEL;type=CELL:+1234567890").unwrap();
        writeln!(temp_file, "EMAIL:john@example.com").unwrap();
        writeln!(temp_file, "END:VCARD").unwrap();
        temp_file.flush().unwrap();

        parser.parse_vcf_file(temp_file.path()).unwrap();

        assert_eq!(parser.get_name_by_phone("+1234567890"), Some(&"John Doe".to_string()));
        assert_eq!(parser.get_name_by_email("john@example.com"), Some(&"John Doe".to_string()));
    }

    #[test]
    fn test_phone_normalization() {
        let parser = VcfParser::new();
        
        // New logic removes country codes for consistent matching
        assert_eq!(parser.normalize_phone_number("+1 (234) 567-8900"), "2345678900");
        assert_eq!(parser.normalize_phone_number("234-567-8900"), "2345678900");
        assert_eq!(parser.normalize_phone_number("(234) 567 8900"), "2345678900");
        assert_eq!(parser.normalize_phone_number("12345678900"), "2345678900");
    }

    #[test]
    fn test_phone_matching() {
        let parser = VcfParser::new();
        
        // Test exact matches after normalization
        assert!(parser.phones_match("+12345678900", "2345678900"));
        assert!(parser.phones_match("2345678900", "+12345678900"));
        
        assert!(parser.phones_match("(234) 567-8900", "+1-234-567-8900"));
        
        // Test that partial matches are rejected (more precise matching)
        assert!(!parser.phones_match("5678900", "2345678900"));
        assert!(!parser.phones_match("234", "2345678900"));
    }

    #[test]
    fn test_empty_name_ignored() {
        let mut parser = VcfParser::new();
        
        // Create a temporary VCF file with no name
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "BEGIN:VCARD").unwrap();
        writeln!(temp_file, "VERSION:3.0").unwrap();
        writeln!(temp_file, "TEL;type=CELL:+1234567890").unwrap();
        writeln!(temp_file, "EMAIL:noreply@example.com").unwrap();
        writeln!(temp_file, "END:VCARD").unwrap();
        temp_file.flush().unwrap();

        parser.parse_vcf_file(temp_file.path()).unwrap();

        assert_eq!(parser.get_name_by_phone("+1234567890"), None);
        assert_eq!(parser.get_name_by_email("noreply@example.com"), None);
    }

    #[test]
    fn test_multiple_phone_numbers_single_contact() {
        let mut parser = VcfParser::new();
        
        // Create a contact with multiple phone numbers
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "BEGIN:VCARD").unwrap();
        writeln!(temp_file, "VERSION:3.0").unwrap();
        writeln!(temp_file, "FN:John Doe").unwrap();
        writeln!(temp_file, "TEL;type=HOME:+1234567890").unwrap();
        writeln!(temp_file, "TEL;type=CELL:+1987654321").unwrap();
        writeln!(temp_file, "TEL;type=WORK:(555) 123-4567").unwrap();
        writeln!(temp_file, "EMAIL:john@home.com").unwrap();
        writeln!(temp_file, "EMAIL:john@work.com").unwrap();
        writeln!(temp_file, "END:VCARD").unwrap();
        temp_file.flush().unwrap();

        parser.parse_vcf_file(temp_file.path()).unwrap();

        // All phone numbers should map to the same contact
        assert_eq!(parser.get_name_by_phone("+1234567890"), Some(&"John Doe".to_string()));
        assert_eq!(parser.get_name_by_phone("+1987654321"), Some(&"John Doe".to_string()));
        assert_eq!(parser.get_name_by_phone("5551234567"), Some(&"John Doe".to_string()));
        
        // All emails should map to the same contact
        assert_eq!(parser.get_name_by_email("john@home.com"), Some(&"John Doe".to_string()));
        assert_eq!(parser.get_name_by_email("john@work.com"), Some(&"John Doe".to_string()));
    }

    #[test]
    fn test_duplicate_contact_merging() {
        let mut parser = VcfParser::new();
        
        // Create VCF with duplicate contacts (like Hannah Agnew in the real file)
        let mut temp_file = NamedTempFile::new().unwrap();
        
        // First Hannah entry - just phone
        writeln!(temp_file, "BEGIN:VCARD").unwrap();
        writeln!(temp_file, "VERSION:3.0").unwrap();
        writeln!(temp_file, "FN:Hannah Agnew").unwrap();
        writeln!(temp_file, "TEL;type=pref:+19059143363").unwrap();
        writeln!(temp_file, "END:VCARD").unwrap();
        
        // Second Hannah entry - email and same phone
        writeln!(temp_file, "BEGIN:VCARD").unwrap();
        writeln!(temp_file, "VERSION:3.0").unwrap();
        writeln!(temp_file, "FN:Hannah Agnew").unwrap();
        writeln!(temp_file, "EMAIL;type=INTERNET:hannersthenanners@hotmail.com").unwrap();
        writeln!(temp_file, "TEL;type=pref:+19059143363").unwrap();
        writeln!(temp_file, "END:VCARD").unwrap();
        
        temp_file.flush().unwrap();

        parser.parse_vcf_file(temp_file.path()).unwrap();

        // Both phone and email should resolve to Hannah
        assert_eq!(parser.get_name_by_phone("+19059143363"), Some(&"Hannah Agnew".to_string()));
        assert_eq!(parser.get_name_by_email("hannersthenanners@hotmail.com"), Some(&"Hannah Agnew".to_string()));
        
        // Should not have duplicate phone numbers in the lookup
        let phone_entries: Vec<_> = parser.phone_to_name.iter()
            .filter(|(_, name)| *name == "Hannah Agnew")
            .collect();
        assert_eq!(phone_entries.len(), 1); // Only one phone entry despite appearing twice
    }

    #[test]
    fn test_duplicate_contact_merging_different_info() {
        let mut parser = VcfParser::new();
        
        // Create VCF with same person having different contact methods in separate entries
        let mut temp_file = NamedTempFile::new().unwrap();
        
        // First entry - home phone
        writeln!(temp_file, "BEGIN:VCARD").unwrap();
        writeln!(temp_file, "VERSION:3.0").unwrap();
        writeln!(temp_file, "FN:Alice Smith").unwrap();
        writeln!(temp_file, "TEL;type=HOME:+1234567890").unwrap();
        writeln!(temp_file, "END:VCARD").unwrap();
        
        // Second entry - work email
        writeln!(temp_file, "BEGIN:VCARD").unwrap();
        writeln!(temp_file, "VERSION:3.0").unwrap();
        writeln!(temp_file, "FN:Alice Smith").unwrap();
        writeln!(temp_file, "EMAIL;type=WORK:alice@company.com").unwrap();
        writeln!(temp_file, "END:VCARD").unwrap();
        
        // Third entry - cell phone
        writeln!(temp_file, "BEGIN:VCARD").unwrap();
        writeln!(temp_file, "VERSION:3.0").unwrap();
        writeln!(temp_file, "FN:Alice Smith").unwrap();
        writeln!(temp_file, "TEL;type=CELL:+1987654321").unwrap();
        writeln!(temp_file, "END:VCARD").unwrap();
        
        temp_file.flush().unwrap();

        parser.parse_vcf_file(temp_file.path()).unwrap();

        // All contact methods should resolve to Alice
        assert_eq!(parser.get_name_by_phone("+1234567890"), Some(&"Alice Smith".to_string()));
        assert_eq!(parser.get_name_by_phone("+1987654321"), Some(&"Alice Smith".to_string()));
        assert_eq!(parser.get_name_by_email("alice@company.com"), Some(&"Alice Smith".to_string()));
        
        // Should have merged all contact methods
        let alice_phone_entries: Vec<_> = parser.phone_to_name.iter()
            .filter(|(_, name)| *name == "Alice Smith")
            .collect();
        let alice_email_entries: Vec<_> = parser.email_to_name.iter()
            .filter(|(_, name)| *name == "Alice Smith")
            .collect();
            
        assert_eq!(alice_phone_entries.len(), 2); // Two phone numbers
        assert_eq!(alice_email_entries.len(), 1); // One email
    }

    #[test]
    fn test_prefixed_email_parsing() {
        let mut temp_file = NamedTempFile::new().unwrap();
        
        // Test VCF with prefixed email field (like item1.EMAIL)
        writeln!(temp_file, "BEGIN:VCARD").unwrap();
        writeln!(temp_file, "VERSION:3.0").unwrap();
        writeln!(temp_file, "FN:Test User").unwrap();
        writeln!(temp_file, "item1.EMAIL;type=INTERNET;type=pref:test@example.com").unwrap();
        writeln!(temp_file, "END:VCARD").unwrap();
        temp_file.flush().unwrap();
        
        let mut parser = VcfParser::new();
        parser.parse_vcf_file(temp_file.path()).unwrap();
        
        // Should find the email even with prefix
        assert_eq!(parser.get_name_by_email("test@example.com"), Some(&"Test User".to_string()));
        assert_eq!(parser.get_name_by_email("TEST@EXAMPLE.COM"), Some(&"Test User".to_string()));
    }

    #[test]
    fn test_email_case_insensitive() {
        let mut temp_file = NamedTempFile::new().unwrap();
        
        writeln!(temp_file, "BEGIN:VCARD").unwrap();
        writeln!(temp_file, "VERSION:3.0").unwrap();
        writeln!(temp_file, "FN:Case Test").unwrap();
        writeln!(temp_file, "EMAIL:MixedCase@Example.COM").unwrap();
        writeln!(temp_file, "END:VCARD").unwrap();
        temp_file.flush().unwrap();
        
        let mut parser = VcfParser::new();
        parser.parse_vcf_file(temp_file.path()).unwrap();
        
        // All these should match
        assert_eq!(parser.get_name_by_email("mixedcase@example.com"), Some(&"Case Test".to_string()));
        assert_eq!(parser.get_name_by_email("MIXEDCASE@EXAMPLE.COM"), Some(&"Case Test".to_string()));
        assert_eq!(parser.get_name_by_email("MixedCase@Example.COM"), Some(&"Case Test".to_string()));
    }

    #[test]
    fn test_email_without_name() {
        let mut temp_file = NamedTempFile::new().unwrap();
        
        // Test VCF with email but no FN field (like in the real VCF file)
        writeln!(temp_file, "BEGIN:VCARD").unwrap();
        writeln!(temp_file, "VERSION:3.0").unwrap();
        writeln!(temp_file, "N:;;;;").unwrap();
        writeln!(temp_file, "EMAIL;type=INTERNET;type=pref:orphan@example.com").unwrap();
        writeln!(temp_file, "END:VCARD").unwrap();
        temp_file.flush().unwrap();
        
        let mut parser = VcfParser::new();
        parser.parse_vcf_file(temp_file.path()).unwrap();
        
        // Should NOT find the email since there's no proper name (FN field)
        assert_eq!(parser.get_name_by_email("orphan@example.com"), None);
    }
}
