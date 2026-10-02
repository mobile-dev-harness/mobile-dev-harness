use super::extract;
use crate::model::{DeclKind, Receiver, RefKind};

fn refs(src: &str) -> Vec<String> {
    let f = extract("a/B.kt", src);
    f.refs
        .iter()
        .map(|r| {
            let from = r.from.map(|i| f.decls[i].key.clone()).unwrap_or_default();
            let receiver = match &r.receiver {
                Receiver::Implicit => String::new(),
                Receiver::Type(t) => format!("{t}."),
                Receiver::Unknown => "?.".into(),
            };
            let kind = match &r.kind {
                RefKind::Resource(t) => format!("R.{t}"),
                k => format!("{k:?}"),
            };
            let trigger = r
                .trigger
                .as_ref()
                .map(|t| format!(" [{t}]"))
                .unwrap_or_default();
            format!("{from}: {kind} {receiver}{}{trigger}", r.name)
        })
        .collect()
}

#[test]
fn declarations_with_owners_signatures_and_annotations() {
    let f = extract(
        "a/B.kt",
        r#"package dev.x
import a.b.C
import a.b.*

class Foo(private val vm: LoginViewModel) : Base(), Cb {
    private val repo: Repo = Repo()
    override fun onCreate(b: Bundle?) { repo.load(1) }
    fun pay(cartId: String, retry: Boolean = false) {}
    companion object { const val K = 1 }
}

@Composable
fun HomeScreen(modifier: Modifier = Modifier) {}

fun String.shout() = uppercase()
"#,
    );
    assert_eq!(f.package, "dev.x");
    assert_eq!(f.imports, ["a.b.C", "a.b.*"]);
    let keys: Vec<(&str, DeclKind)> = f.decls.iter().map(|d| (d.key.as_str(), d.kind)).collect();
    assert_eq!(
        keys,
        [
            ("Foo", DeclKind::Class),
            ("Foo.repo", DeclKind::Property),
            ("Foo.onCreate", DeclKind::Function),
            ("Foo.pay", DeclKind::Function),
            ("Foo.Companion", DeclKind::Object),
            ("Foo.Companion.K", DeclKind::Property),
            ("HomeScreen", DeclKind::Function),
            ("shout", DeclKind::Function),
        ]
    );
    assert_eq!(f.decls[0].supertypes, ["Base", "Cb"]);
    assert!(f.decls[2].overrides);
    assert_eq!(f.decls[3].arity, Some((1, Some(2))));
    assert_eq!(
        f.decls[3].params.as_deref(),
        Some("(cartId: String, retry: Boolean = false)")
    );
    assert!(f.decls[6].is_composable());
    assert_eq!(f.decls[7].extends.as_deref(), Some("String"));
    assert_eq!(f.errors, 0);
}

#[test]
fn bodies_ignore_comments_and_formatting() {
    let a = extract("a/B.kt", "fun f() {\n    g(1) // call\n}\n");
    let b = extract("a/B.kt", "fun f() { /* note */ g(1) }");
    let c = extract("a/B.kt", "fun f() { g(2) }");
    let d = extract("a/B.kt", "fun f(x: Int) { g(1) }");
    assert_eq!(a.decls[0].body, b.decls[0].body);
    assert_eq!(a.decls[0].signature, b.decls[0].signature);
    assert_ne!(a.decls[0].body, c.decls[0].body);
    assert_ne!(a.decls[0].signature, d.decls[0].signature);
    assert_eq!(a.decls[0].body, d.decls[0].body);
}

#[test]
fn references_with_receivers() {
    let r = refs(
        r#"class A(private val vm: LoginViewModel) {
    private val repo = Repo()
    fun f() {
        Checkout().pay(cartId = "")
        repo.load(1)
        vm.signIn()
        it.unknown()
        Log.e("T", "m")
        helper()
        val x = Config.TIMEOUT
    }
}"#,
    );
    for expected in [
        "A.repo: Call Repo",
        "A.f: Call Checkout.pay",
        "A.f: Call Checkout",
        "A.f: Call Repo.load",
        "A.f: Call LoginViewModel.signIn",
        "A.f: Call ?.unknown",
        "A.f: Call Log.e",
        "A.f: Call helper",
        "A.f: Name Config.TIMEOUT",
    ] {
        assert!(
            r.contains(&expected.to_string()),
            "missing {expected:?} in {r:#?}"
        );
    }
    // Named arguments and parameter names aren't uses.
    assert!(!r.iter().any(|l| l.ends_with(" cartId")), "{r:#?}");
}

#[test]
fn resources_class_literals_and_triggers() {
    let r = refs(
        r#"class Main : ComponentActivity() {
    override fun onCreate(s: Bundle?) {
        setContentView(R.layout.activity_main)
        val signIn = findViewById<Button>(R.id.sign_in)
        mapOf(R.id.open_login to LoginActivity::class.java).forEach { (id, screen) -> open(id, screen) }
        signIn.setOnClickListener { startActivity(Intent(this, MessagesActivity::class.java)) }
        findViewById<Button>(R.id.help).setOnClickListener { startActivity(Intent(this, HelpActivity::class.java)) }
        title = getString(android.R.string.ok)
        val b = ActivityMainBinding.inflate(layoutInflater)
    }

    private fun open(id: Int, screen: Class<out Activity>) {}
}"#,
    );
    for expected in [
        "Main.onCreate: R.layout activity_main",
        "Main.onCreate: ClassLiteral LoginActivity [open_login]",
        "Main.onCreate: ClassLiteral MessagesActivity [sign_in]",
        "Main.onCreate: ClassLiteral HelpActivity [help]",
        "Main.onCreate: Call open",
        "Main.onCreate: R.layout activity_main",
    ] {
        assert!(
            r.contains(&expected.to_string()),
            "missing {expected:?} in {r:#?}"
        );
    }
    assert!(
        !r.iter().any(|l| l.contains(" ok")),
        "framework resources are not the app's: {r:#?}"
    );
    assert!(!r.iter().any(|l| l.ends_with(" java")), "{r:#?}");
}

#[test]
fn calls_to_soft_keyword_names_keep_the_structure() {
    let f = extract(
        "a/B.kt",
        "class Main {\n    fun a() { open(1, 2) }\n    private fun open(id: Int, x: Int) {}\n}\n",
    );
    let keys: Vec<&str> = f.decls.iter().map(|d| d.key.as_str()).collect();
    assert_eq!(keys, ["Main", "Main.a", "Main.open"]);
    assert_eq!(f.errors, 0);
}

#[test]
fn overloads_get_distinct_keys() {
    let f = extract(
        "a/B.kt",
        "fun f(a: Int) {}\nfun f(a: String) {}\nfun g() {}\n",
    );
    let keys: Vec<&str> = f.decls.iter().map(|d| d.key.as_str()).collect();
    assert_eq!(keys, ["f(a: Int)", "f(a: String)", "g"]);
}

#[test]
fn locals_are_not_declarations() {
    let f = extract(
        "a/B.kt",
        "fun f() {\n    val x = 1\n    fun inner() = x\n    class L\n}\n",
    );
    let keys: Vec<&str> = f.decls.iter().map(|d| d.key.as_str()).collect();
    assert_eq!(keys, ["f"]);
}

#[test]
fn string_literals_name_assets() {
    let r = refs("fun f() { load(\"file:///android_asset/page.html\") }");
    assert!(r.contains(&"f: Literal page.html".to_string()), "{r:#?}");
}
