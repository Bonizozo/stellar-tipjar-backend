use tera::{Context, Tera};
use lazy_static::lazy_static;
use std::collections::HashMap;

lazy_static! {
    pub static ref TERA: Tera = {
        let mut tera = match Tera::new("templates/**/*") {
            Ok(t) => t,
            Err(e) => {
                tracing::error!("Tera error: {}", e);
                std::process::exit(1);
            }
        };
        tera.autoescape_on(vec![".html", ".htm", ".xml"]);
        tera
    };
}

pub fn render_template(template_name: &str, variables: &HashMap<&str, String>) -> anyhow::Result<String> {
    let mut context = Context::new();
    for (k, v) in variables {
        context.insert(*k, v);
    }
    
    match TERA.render(template_name, &context) {
        Ok(s) => Ok(s),
        Err(e) => {
            tracing::error!("Failed to render template {}: {}", template_name, e);
            anyhow::bail!("Template error: {}", e)
        }
    }
}
