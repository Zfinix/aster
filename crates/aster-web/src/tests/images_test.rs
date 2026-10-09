use serde_json::json;

use super::*;

#[test]
fn commons_hits_keep_search_rank_and_name_the_file_once() {
    let body = json!({ "query": { "pages": {
        "900": { "index": 2, "title": "File:Alfama rooftops.jpg", "imageinfo": [{
            "thumburl": "https://upload.wikimedia.org/thumb/alfama-800.jpg",
            "url": "https://upload.wikimedia.org/alfama.jpg",
            "descriptionurl": "https://commons.wikimedia.org/wiki/File:Alfama_rooftops.jpg",
            "extmetadata": { "LicenseShortName": { "value": "CC BY-SA 4.0" } }
        }]},
        "100": { "index": 1, "title": "File:Torre de Belém.jpeg", "imageinfo": [{
            "thumburl": "https://upload.wikimedia.org/thumb/belem-800.jpeg",
            "descriptionurl": "https://commons.wikimedia.org/wiki/File:Torre_de_Bel%C3%A9m.jpeg",
            "extmetadata": {
                "Artist": { "value": "<a href=\"//commons.wikimedia.org/wiki/User:Jo\">Jo   Silva</a>" },
                "LicenseShortName": { "value": "CC BY 3.0" }
            }
        }]},
        "300": { "index": 3, "title": "File:No info.png" }
    }}});
    assert_eq!(
        commons_hits(&body),
        [
            ImageHit {
                url: "https://commons.wikimedia.org/wiki/Special:FilePath/Torre_de_Belém.jpeg?width=800".into(),
                page: "https://commons.wikimedia.org/wiki/File:Torre_de_Bel%C3%A9m.jpeg".into(),
                title: "Torre de Belém".into(),
                credit: Some("Jo Silva".into()),
                license: Some("CC BY 3.0".into()),
                source: "Wikimedia Commons",
            },
            ImageHit {
                url: "https://commons.wikimedia.org/wiki/Special:FilePath/Alfama_rooftops.jpg?width=800".into(),
                page: "https://commons.wikimedia.org/wiki/File:Alfama_rooftops.jpg".into(),
                title: "Alfama rooftops".into(),
                credit: None,
                license: Some("CC BY-SA 4.0".into()),
                source: "Wikimedia Commons",
            },
        ]
    );
}

#[test]
fn openverse_hits_name_the_license() {
    let body = json!({ "results": [
        {
            "url": "https://live.staticflickr.com/belem.jpg",
            "foreign_landing_url": "https://www.flickr.com/photos/x/1",
            "title": "Belem",
            "creator": "maria",
            "license": "by",
            "license_version": "2.0"
        },
        { "title": "missing urls" }
    ]});
    assert_eq!(
        openverse_hits(&body),
        [ImageHit {
            url: "https://live.staticflickr.com/belem.jpg".into(),
            page: "https://www.flickr.com/photos/x/1".into(),
            title: "Belem".into(),
            credit: Some("maria".into()),
            license: Some("CC BY 2.0".into()),
            source: "Openverse",
        }]
    );
}

#[test]
fn a_long_credit_is_clipped() {
    let long = "a".repeat(500);
    let clipped = credit(Some(&long)).unwrap();
    assert_eq!(clipped.chars().count(), MAX_CREDIT_CHARS + 1);
}
