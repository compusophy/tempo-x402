pub mod pricing {
    pub fn get_tier_price(slug: &str) -> (String, String) {
        match slug {
            "soul-analysis" | "mission-report" | "financial-audit" => ("$0.005".to_string(), "5000".to_string()),
            "code-audit" | "security-scan" => ("$0.01".to_string(), "10000".to_string()),
            _ => ("$0.001".to_string(), "1000".to_string()),
        }
    }
}
