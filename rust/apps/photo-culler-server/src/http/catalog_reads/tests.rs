use super::*;
use serde_json::{json, to_value};
use std::collections::BTreeMap;

struct Fake;
fn photo(id: &str, gallery: &str, decision: &str) -> Photo {
    Photo {
        id: id.into(),
        gallery_id: gallery.into(),
        name: format!("photo {id}"),
        decision: decision.into(),
        score: Some(0.5),
        quality_tier: None,
        analysis_summary: json!({"sharpness": 0.8}),
        metadata: json!({"camera_model": "camera"}),
    }
}
impl Catalog for Fake {
    fn galleries(&self) -> Result<Vec<Gallery>, Error> {
        Ok(vec![
            Gallery {
                id: "b".into(),
                name: "B".into(),
            },
            Gallery {
                id: "a".into(),
                name: "A".into(),
            },
        ])
    }
    fn active_gallery(&self) -> Result<Option<String>, Error> {
        Ok(Some("a".into()))
    }
    fn photos(&self, _: &Filter) -> Result<Vec<Photo>, Error> {
        Ok(vec![
            photo("3", "a", "KEEP"),
            photo("2", "b", "KEEP"),
            photo("4", "a", "REJECT"),
            photo("1", "a", "KEEP"),
        ])
    }
    fn photo(&self, id: &str) -> Result<Option<Photo>, Error> {
        Ok((id == "1").then(|| photo("1", "a", "KEEP")))
    }
    fn sessions(&self, _: &str) -> Result<Vec<Session>, Error> {
        Ok(vec![Session {
            id: "s".into(),
            name: "session".into(),
            photo_count: 2,
        }])
    }
    fn groups(&self, _: &str) -> Result<Vec<Group>, Error> {
        Ok(vec![Group {
            id: "g".into(),
            photo_count: 2,
            recommended_photo_id: "1".into(),
        }])
    }
    fn profiles(&self) -> Result<Vec<Profile>, Error> {
        Ok(vec![Profile {
            id: "p".into(),
            name: "profile".into(),
        }])
    }
    fn summary(&self, _: &str) -> Result<Summary, Error> {
        Ok(Summary {
            total_photos: 3,
            total_files: 4,
            selected_count: 2,
            pending_count: 0,
            decisions: BTreeMap::from([("KEEP".into(), 2), ("REJECT".into(), 1)]),
            quality_tiers: BTreeMap::new(),
        })
    }
    fn thumbnail(&self, id: &str, _: u32) -> Result<Option<Thumbnail>, Error> {
        Ok((id == "1").then(|| Thumbnail {
            content_type: "image/jpeg".into(),
            bytes: vec![0xff, 0xd8, 0xff, 0xd9],
        }))
    }
}
#[test]
fn two_galleries_have_reproducible_filtered_pages() {
    let reads = Reads(Fake);
    assert_eq!(reads.active_gallery().unwrap(), Some("a".into()));
    assert_eq!(
        reads
            .galleries()
            .unwrap()
            .items
            .iter()
            .map(|g| &*g.id)
            .collect::<Vec<_>>(),
        vec!["a", "b"]
    );
    let filter = Filter {
        gallery_id: "a".into(),
        decision: Some("KEEP".into()),
    };
    let first = reads.page(filter.clone(), None, 1).unwrap();
    assert_eq!(first, reads.page(filter.clone(), None, 1).unwrap());
    assert_eq!(first.items[0].id, "1");
    let cursor = first.next_cursor.unwrap();
    assert_eq!(cursor.filter, filter);
    let second = reads.page(filter.clone(), Some(cursor.clone()), 1).unwrap();
    assert_eq!(second.items[0].id, "3");
    assert!(second.next_cursor.is_none());
    assert_eq!(
        reads.page(
            Filter {
                gallery_id: "b".into(),
                decision: filter.decision
            },
            Some(cursor),
            1
        ),
        Err(Error::CursorFilterMismatch)
    );
    assert_eq!(
        reads.page(
            Filter {
                gallery_id: "a".into(),
                decision: None
            },
            None,
            0
        ),
        Err(Error::InvalidPage)
    );
}
#[test]
fn detail_and_summary_keep_the_legacy_json_shape() {
    let reads = Reads(Fake);
    assert_eq!(
        to_value(reads.detail("1").unwrap()).unwrap(),
        json!({"contract_version":1,"id":"1","name":"photo 1","decision":"KEEP","score":0.5,"quality_tier":null,"analysis_summary":{"sharpness":0.8},"metadata":{"camera_model":"camera"}})
    );
    assert_eq!(
        to_value(reads.summary("a").unwrap()).unwrap(),
        json!({"total_photos":3,"total_files":4,"selected_count":2,"pending_count":0,"decisions":{"KEEP":2,"REJECT":1},"quality_tiers":{}})
    );
    assert_eq!(reads.detail("missing"), Err(Error::NotFound));
}
#[test]
fn other_read_ports_and_thumbnail_are_available() {
    let reads = Reads(Fake);
    assert_eq!(reads.sessions("a").unwrap().items[0].id, "s");
    assert_eq!(
        reads.groups("a").unwrap().items[0].recommended_photo_id,
        "1"
    );
    assert_eq!(reads.profiles().unwrap().items[0].id, "p");
    assert_eq!(
        reads.thumbnail("1", 800).unwrap().content_type,
        "image/jpeg"
    );
    assert_eq!(reads.thumbnail("missing", 800), Err(Error::NotFound));
    assert_eq!(reads.thumbnail("1", 0), Err(Error::InvalidPage));
}
