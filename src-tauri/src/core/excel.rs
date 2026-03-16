use std::collections::HashMap;
use std::fs::File;
use std::io::Read;

use regex::Regex;
use zip::ZipArchive;

pub fn extract_wps_images(excel_path: &str) -> Result<HashMap<String, Vec<u8>>, String> {
    let file =
        File::open(excel_path).map_err(|e| format!("open excel for image extract failed: {e}"))?;
    let mut archive = ZipArchive::new(file).map_err(|e| format!("parse excel zip failed: {e}"))?;

    let mut rels_content = String::new();
    if let Ok(mut f) = archive.by_name("xl/_rels/cellimages.xml.rels") {
        let _ = f.read_to_string(&mut rels_content);
    }
    let mut rid_to_target = HashMap::new();
    let re_rel = Regex::new(r#"Id="([^"]+)"[^>]*Target="([^"]+)""#)
        .map_err(|e| format!("compile relation regex failed: {e}"))?;
    for cap in re_rel.captures_iter(&rels_content) {
        rid_to_target.insert(cap[1].to_string(), cap[2].to_string());
    }

    let mut cellimages_content = String::new();
    if let Ok(mut f) = archive.by_name("xl/cellimages.xml") {
        let _ = f.read_to_string(&mut cellimages_content);
    }

    let mut id_to_target = HashMap::new();
    let re_name =
        Regex::new(r#"name="([^"]+)""#).map_err(|e| format!("compile name regex failed: {e}"))?;
    let re_embed = Regex::new(r#"r:embed="([^"]+)""#)
        .map_err(|e| format!("compile embed regex failed: {e}"))?;
    for block in cellimages_content.split("<etc:cellImage>") {
        if let (Some(cap_name), Some(cap_embed)) =
            (re_name.captures(block), re_embed.captures(block))
        {
            if let Some(target) = rid_to_target.get(&cap_embed[1]) {
                id_to_target.insert(cap_name[1].to_string(), target.clone());
            }
        }
    }

    let mut image_data = HashMap::new();
    for (id, target) in id_to_target {
        let clean_target = if target.starts_with("../") {
            format!("xl/{}", &target[3..])
        } else {
            format!("xl/{target}")
        };

        if let Ok(mut f) = archive.by_name(&clean_target) {
            let mut buf = Vec::new();
            if f.read_to_end(&mut buf).is_ok() {
                image_data.insert(id, buf);
            }
        }
    }

    Ok(image_data)
}
