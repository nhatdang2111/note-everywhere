//! CSV import and export with synthetic fixtures.

mod support;

use ne_vault::import::{ImportError, detect_format, parse_csv};
use ne_vault::{
    ColumnMapping, CsvFormat, Field, ItemBody, ItemKind, SkipReason, Vault, VaultError,
};
use support::*;

const T0: i64 = 1_700_000_000_000;

fn vault() -> Vault {
    unlocked_replica(&account(), TestClock::at(T0))
}

fn skipped(report: &ne_vault::ImportReport) -> Vec<(usize, SkipReason)> {
    report.skipped.iter().map(|s| (s.row, s.reason)).collect()
}

const BITWARDEN: &str = "folder,favorite,type,name,notes,fields,reprompt,login_uri,login_username,login_password,login_totp\r\n\
Work,1,login,Example Site,\"first line\nsecond line\",\"PIN: 1234\",0,\"https://one.example.test\nhttps://two.example.test\",ana,\"p,w\"\"1\",JBSWY3DPEHPK3PXP\r\n\
,,note,Wifi code,\"router password is synthetic-wifi\",,0,,,,\r\n\
,,card,Some card,,,0,,,,\r\n\
\r\n\
,,login,,,,0,https://only-url.example.test,bob,pw-bob,\r\n\
,,login,,,,0,,,,\r\n";

#[test]
fn bitwarden_layout() {
    let mut vault = vault();
    let report = vault
        .import_csv(BITWARDEN.as_bytes(), &CsvFormat::Bitwarden)
        .expect("import");
    assert_eq!(report.imported, 3);
    assert_eq!(
        skipped(&report),
        [(3, SkipReason::UnsupportedType), (5, SkipReason::Empty)]
    );
    let items = vault.list().expect("list");
    let by_title = |t: &str| items.iter().find(|i| i.title == t).expect("item").id;

    let site = by_title("Example Site");
    assert_eq!(vault.get(&site).expect("get").tags, ["Work"]);
    assert_eq!(
        vault.get(&site).expect("get").urls,
        ["https://one.example.test", "https://two.example.test"]
    );
    assert_eq!(&*vault.reveal(&site, Field::Password).expect("r"), "p,w\"1");
    let notes = vault.reveal(&site, Field::Notes).expect("r");
    assert_eq!(
        &*notes,
        "first line\nsecond line\nfields: PIN: 1234\nlogin_totp: JBSWY3DPEHPK3PXP"
    );

    let wifi = by_title("Wifi code");
    assert_eq!(vault.get(&wifi).expect("get").kind, ItemKind::SecureNote);
    assert_eq!(
        &*vault.reveal(&wifi, Field::Text).expect("r"),
        "router password is synthetic-wifi"
    );

    // A login without a title takes its URL as the title.
    assert_eq!(
        by_title("https://only-url.example.test"),
        items
            .iter()
            .find(|i| i.username.as_deref() == Some("bob"))
            .expect("bob")
            .id
    );
}

#[test]
fn lastpass_layout_with_bom_notes_and_old_header() {
    let current = "\u{feff}url,username,password,totp,extra,name,grouping,fav\r\n\
https://shop.example.test,carol,pw-carol,,\"multi\nline note\",Shop,Family,0\r\n\
http://sn,,,,\"synthetic secure note text\",Safe code,Family,1\r\n";
    let mut vault = vault();
    let report = vault
        .import_csv(current.as_bytes(), &CsvFormat::LastPass)
        .expect("import");
    assert_eq!((report.imported, report.skipped.len()), (2, 0));
    let items = vault.list().expect("list");
    let shop = items.iter().find(|i| i.title == "Shop").expect("shop");
    assert_eq!(shop.tags, ["Family"]);
    assert_eq!(
        &*vault.reveal(&shop.id, Field::Notes).expect("r"),
        "multi\nline note"
    );
    let note = items.iter().find(|i| i.title == "Safe code").expect("note");
    assert_eq!(note.kind, ItemKind::SecureNote);
    assert_eq!(
        &*vault.reveal(&note.id, Field::Text).expect("r"),
        "synthetic secure note text"
    );

    let old = "url,username,password,extra,name,grouping,fav\nhttps://a.example.test,dave,pw-dave,,Old export,,0\n";
    let mut other = self::vault();
    assert_eq!(
        other
            .import_csv(old.as_bytes(), &CsvFormat::LastPass)
            .expect("import")
            .imported,
        1
    );
}

#[test]
fn onepassword_layout_with_url_or_website_column() {
    let url = "Title,Url,Username,Password,OTPAuth,Favorite,Archived,Tags,Notes\n\
Cloud,https://cloud.example.test,erin,pw-erin,otpauth://totp/x?secret=ABC,false,false,\"infra, prod\",\"note, with comma\"\n";
    let website =
        "Title,Website,Username,Password,Notes\nCloud,https://cloud.example.test,erin,pw-erin,\n";
    for csv in [url, website] {
        let mut vault = vault();
        let report = vault
            .import_csv(csv.as_bytes(), &CsvFormat::OnePassword)
            .expect("import");
        assert_eq!(report.imported, 1);
        let item = vault.list().expect("list").remove(0);
        assert_eq!(item.urls, ["https://cloud.example.test"]);
        assert_eq!(item.username.as_deref(), Some("erin"));
    }
    let mut vault = vault();
    vault
        .import_csv(url.as_bytes(), &CsvFormat::OnePassword)
        .expect("import");
    let item = vault.list().expect("list").remove(0);
    assert_eq!(item.tags, ["infra", "prod"]);
    assert_eq!(
        &*vault.reveal(&item.id, Field::Notes).expect("r"),
        "note, with comma\nOTPAuth: otpauth://totp/x?secret=ABC"
    );
}

#[test]
fn generic_mapping_names_columns_and_is_strict_about_them() {
    let csv = "Site name;ignored\n"; // wrong delimiter: one column
    assert!(
        parse_csv(
            csv.as_bytes(),
            &CsvFormat::Generic(ColumnMapping::vault_export())
        )
        .is_err()
    );

    let csv =
        "Name,Login,Secret,Kind\nRouter,admin,pw-router,\nToken A,,tok-a,apikey\nMemo,,,note\n";
    let mapping = ColumnMapping {
        title: "name".into(),
        username: Some("LOGIN".into()),
        password: Some("secret".into()),
        url: None,
        notes: None,
        tags: None,
        kind: Some("kind".into()),
    };
    let batch = parse_csv(csv.as_bytes(), &CsvFormat::Generic(mapping.clone())).expect("parse");
    assert_eq!(batch.report.imported, 3);
    assert!(matches!(batch.drafts[1].body, ItemBody::ApiKey { .. }));
    assert!(matches!(batch.drafts[2].body, ItemBody::SecureNote { .. }));

    let typo = ColumnMapping {
        password: Some("pasword".into()),
        ..mapping
    };
    assert_eq!(
        parse_csv(csv.as_bytes(), &CsvFormat::Generic(typo)).err(),
        Some(ImportError::MissingColumn("pasword".into()))
    );
}

#[test]
fn file_level_errors() {
    let bitwarden = CsvFormat::Bitwarden;
    assert_eq!(
        parse_csv(b"", &bitwarden).err(),
        Some(ImportError::NoHeader)
    );
    assert_eq!(
        parse_csv(b"\n\n", &bitwarden).err(),
        Some(ImportError::NoHeader)
    );
    assert_eq!(
        parse_csv(&[0xff, 0xfe, b'n', 0], &bitwarden).err(),
        Some(ImportError::UnsupportedEncoding)
    );
    assert_eq!(
        parse_csv(&[0x80, 0x81], &bitwarden).err(),
        Some(ImportError::UnsupportedEncoding)
    );
    assert_eq!(
        parse_csv(b"a,b\n1,2\n", &bitwarden).err(),
        Some(ImportError::MissingColumn("name".into()))
    );
    let mut vault = vault();
    assert!(matches!(
        vault.import_csv(b"a,b\n", &CsvFormat::Bitwarden),
        Err(VaultError::Import(_))
    ));
    vault.lock();
    assert_eq!(
        vault
            .import_csv(BITWARDEN.as_bytes(), &CsvFormat::Bitwarden)
            .err(),
        Some(VaultError::Locked)
    );
}

#[test]
fn bad_rows_are_reported_and_good_rows_still_import() {
    let title_only = |extra: &[&str]| ColumnMapping {
        title: "title".into(),
        username: extra.contains(&"username").then(|| "username".into()),
        password: extra.contains(&"password").then(|| "password".into()),
        url: None,
        notes: None,
        tags: None,
        kind: None,
    };
    let csv = "title,username,password\nGood,u,p\n,,\nNo password row,u2,\n,u3,p3\n";
    let batch = parse_csv(
        csv.as_bytes(),
        &CsvFormat::Generic(title_only(&["username", "password"])),
    )
    .expect("parse");
    assert_eq!(batch.report.imported, 2);
    assert_eq!(
        skipped(&batch.report),
        [(2, SkipReason::Empty), (4, SkipReason::MissingTitle)]
    );

    // A field that opens a quote and never closes it is reported, not guessed.
    let open = "title\nfine\n\"unterminated";
    let batch = parse_csv(open.as_bytes(), &CsvFormat::Generic(title_only(&[]))).expect("parse");
    assert_eq!(batch.report.imported, 1);
    assert_eq!(skipped(&batch.report), [(2, SkipReason::Malformed)]);
}

#[test]
fn reports_and_errors_never_contain_cell_contents() {
    let secret = "synthetic-super-secret-value";
    let csv = format!(
        "name,login_username,login_password,type,login_uri\n{secret},{secret},{secret},card,{secret}\n"
    );
    let batch = parse_csv(csv.as_bytes(), &CsvFormat::Bitwarden).expect("parse");
    let text = format!("{:?} {:?}", batch.report, batch);
    assert!(!text.contains(secret));
    let rendered: String = batch
        .report
        .skipped
        .iter()
        .map(|s| s.reason.to_string())
        .collect();
    assert!(!rendered.contains(secret));
    let err = parse_csv(format!("{secret},x\n").as_bytes(), &CsvFormat::Bitwarden)
        .expect_err("missing columns");
    assert!(!err.to_string().contains(secret));
}

#[test]
fn format_detection_reads_the_header_only() {
    assert_eq!(
        detect_format(BITWARDEN.as_bytes()),
        Some(CsvFormat::Bitwarden)
    );
    assert_eq!(
        detect_format(b"\xEF\xBB\xBFurl,username,password,extra,name,grouping,fav\n"),
        Some(CsvFormat::LastPass)
    );
    assert_eq!(
        detect_format(b"Title,Url,Username,Password,OTPAuth,Favorite,Archived,Tags,Notes\n"),
        Some(CsvFormat::OnePassword)
    );
    assert_eq!(detect_format(b"foo,bar\n1,2\n"), None);
    assert_eq!(detect_format(b""), None);
    assert_eq!(detect_format(&[0xff, 0xfe]), None);
}

#[test]
fn export_round_trips_through_generic_import() {
    let mut source = vault();
    let mut tricky = login("Quote \"title\", with comma", "user", "pa\"ss,w\nord");
    tricky.tags = vec!["a".into(), "b".into()];
    if let ItemBody::Login { urls, notes, .. } = &mut tricky.body {
        urls.extend([
            "https://x.example.test".to_owned(),
            "https://y.example.test".to_owned(),
        ]);
        *notes = "line one\nline two".into();
    }
    source.add(tricky).expect("add");
    source.add(note("Memo", "memo body")).expect("add");
    source.add(api_key("Token", "tok_1")).expect("add");
    let csv = source.export_csv().expect("export");
    assert!(csv.starts_with("type,title,username,password,url,notes,tags\r\n"));

    let mut copy = vault();
    let report = copy
        .import_csv(
            csv.as_bytes(),
            &CsvFormat::Generic(ColumnMapping::vault_export()),
        )
        .expect("import");
    assert_eq!((report.imported, report.skipped.len()), (3, 0));
    let original = source.list().expect("list");
    let restored = copy.list().expect("list");
    assert_eq!(
        original
            .iter()
            .map(|i| (&i.title, &i.tags, &i.urls, i.kind))
            .collect::<Vec<_>>(),
        restored
            .iter()
            .map(|i| (&i.title, &i.tags, &i.urls, i.kind))
            .collect::<Vec<_>>()
    );
    let tricky_id = restored[1].id;
    assert_eq!(restored[1].title, "Quote \"title\", with comma");
    assert_eq!(
        &*copy.reveal(&tricky_id, Field::Password).expect("r"),
        "pa\"ss,w\nord"
    );
    assert_eq!(
        &*copy.reveal(&tricky_id, Field::Notes).expect("r"),
        "line one\nline two"
    );
}
