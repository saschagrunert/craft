#[macro_use]
extern crate hamcrest;
extern crate craft;
extern crate crafttest;
extern crate tempdir;

use std::fs::{self, File};
use std::io::prelude::*;

use craft::util::ProcessBuilder;

use crafttest::process;
use crafttest::support::{execs, paths};

use hamcrest::prelude::*;
use tempdir::TempDir;

fn craft_process(s: &str) -> ProcessBuilder {
    let mut p = crafttest::craft_process();
    p.arg(s);
    return p;
}

#[test]
fn simple_lib() {
    assert_that!(craft_process("new").arg("--lib").arg("foo").arg("--vcs").arg("none").env("USER", "foo"),
                 execs().with_status(0).with_stderr("\
[Created] library `foo` project
"));

    assert_that!(&paths::root().join("foo"), existing_dir());
    assert_that!(&paths::root().join("foo/Craft.toml"), existing_file());
    assert_that!(&paths::root().join("foo/src/lib.c"), existing_file());
    assert_that!(&paths::root().join("foo/.gitignore"),
                 is_not(existing_file()));

    assert_that!(craft_process("build").cwd(&paths::root().join("foo")),
                 execs().with_status(0));
}

#[test]
fn simple_bin() {
    assert_that!(craft_process("new").arg("--bin").arg("foo").env("USER", "foo"),
                 execs().with_status(0).with_stderr("\
[Created] binary (application) `foo` project
"));

    assert_that!(&paths::root().join("foo"), existing_dir());
    assert_that!(&paths::root().join("foo/Craft.toml"), existing_file());
    assert_that!(&paths::root().join("foo/src/main.c"), existing_file());
}

#[test]
fn both_lib_and_bin() {
    let td = TempDir::new("craft").unwrap();
    assert_that!(craft_process("new").arg("--lib").arg("--bin").arg("foo").cwd(td.path().clone()).env("USER", "foo"),
                 execs().with_status(101).with_stderr("[Error] can't specify both lib and binary outputs"));
}

#[test]
fn simple_git() {
    let td = TempDir::new("craft").unwrap();
    assert_that!(craft_process("new")
                     .arg("--lib")
                     .arg("foo")
                     .cwd(td.path().clone())
                     .env("USER", "foo"),
                 execs().with_status(0));

    assert_that!(td.path(), existing_dir());
    assert_that!(&td.path().join("foo/Craft.toml"), existing_file());
    assert_that!(&td.path().join("foo/src/lib.c"), existing_file());
    assert_that!(&td.path().join("foo/.git"), existing_dir());
    assert_that!(&td.path().join("foo/.gitignore"), existing_file());

    assert_that!(craft_process("build").cwd(&td.path().clone().join("foo")),
                 execs().with_status(0));
}

#[test]
fn no_argument() {
    assert_that!(craft_process("new"),
                 execs().with_status(1).with_stderr("\
[Error] Invalid arguments.

Usage:
    craft new [options] <path>
    craft \
                                   new -h | --help
"));
}

#[test]
fn existing() {
    let dst = paths::root().join("foo");
    fs::create_dir(&dst).unwrap();
    assert_that!(craft_process("new").arg("foo"),
                 execs()
                     .with_status(101)
                     .with_stderr(format!("[Error] destination `{}` already exists\n", dst.display())));
}

#[test]
fn invalid_characters() {
    assert_that!(craft_process("new").arg("foo.c"),
                 execs().with_status(101).with_stderr("\
[Error] Invalid character `.` in chest name: `foo.c`
use --name to override \
                                   chest name"));
}

#[test]
fn reserved_name() {
    assert_that!(craft_process("new").arg("test"),
                 execs().with_status(101).with_stderr("\
[Error] The name `test` cannot be used as a chest name\nuse --name to override \
                                   chest name"));
}

#[test]
fn keyword_name() {
    assert_that!(craft_process("new").arg("int"),
                 execs().with_status(101).with_stderr("\
[Error] The name `int` cannot be used as a chest name\nuse --name to override chest name"));
}

#[test]
fn bin_disables_stripping() {
    assert_that!(craft_process("new").arg("rust-foo").arg("--bin").env("USER", "foo"),
                 execs().with_status(0));
    let toml = paths::root().join("rust-foo/Craft.toml");
    let mut contents = String::new();
    File::open(&toml).unwrap().read_to_string(&mut contents).unwrap();
    assert!(contents.contains(r#"name = "rust-foo""#));
}

#[test]
fn explicit_name_not_stripped() {
    assert_that!(craft_process("new").arg("foo").arg("--name").arg("rust-bar").env("USER", "foo"),
                 execs().with_status(0));
    let toml = paths::root().join("foo/Craft.toml");
    let mut contents = String::new();
    File::open(&toml).unwrap().read_to_string(&mut contents).unwrap();
    assert!(contents.contains(r#"name = "rust-bar""#));
}

#[test]
fn finds_author_user() {
    // Use a temp dir to make sure we don't pick up .craft/config somewhere in
    // the hierarchy
    let td = TempDir::new("craft").unwrap();
    assert_that!(craft_process("new").arg("foo").env("USER", "foo").cwd(td.path().clone()),
                 execs().with_status(0));

    let toml = td.path().join("foo/Craft.toml");
    let mut contents = String::new();
    File::open(&toml).unwrap().read_to_string(&mut contents).unwrap();
    assert!(contents.contains(r#"authors = ["foo"]"#));
}

#[test]
fn finds_author_user_escaped() {
    // Use a temp dir to make sure we don't pick up .craft/config somewhere in
    // the hierarchy
    let td = TempDir::new("craft").unwrap();
    assert_that!(craft_process("new").arg("foo").env("USER", "foo \"bar\"").cwd(td.path().clone()),
                 execs().with_status(0));

    let toml = td.path().join("foo/Craft.toml");
    let mut contents = String::new();
    File::open(&toml).unwrap().read_to_string(&mut contents).unwrap();
    assert!(contents.contains(r#"authors = ["foo \"bar\""]"#));
}

#[test]
fn finds_author_username() {
    // Use a temp dir to make sure we don't pick up .craft/config somewhere in
    // the hierarchy
    let td = TempDir::new("craft").unwrap();
    assert_that!(craft_process("new").arg("foo").env_remove("USER").env("USERNAME", "foo").cwd(td.path().clone()),
                 execs().with_status(0));

    let toml = td.path().join("foo/Craft.toml");
    let mut contents = String::new();
    File::open(&toml).unwrap().read_to_string(&mut contents).unwrap();
    assert!(contents.contains(r#"authors = ["foo"]"#));
}

#[test]
fn finds_author_priority() {
    // Use a temp dir to make sure we don't pick up .craft/config somewhere in
    // the hierarchy
    let td = TempDir::new("craft").unwrap();
    assert_that!(craft_process("new")
                     .arg("foo")
                     .env("USER", "bar2")
                     .env("EMAIL", "baz2")
                     .env("CRAFT_NAME", "bar")
                     .env("CRAFT_EMAIL", "baz")
                     .cwd(td.path().clone()),
                 execs().with_status(0));

    let toml = td.path().join("foo/Craft.toml");
    let mut contents = String::new();
    File::open(&toml).unwrap().read_to_string(&mut contents).unwrap();
    assert!(contents.contains(r#"authors = ["bar <baz>"]"#));
}

#[test]
fn finds_author_email() {
    // Use a temp dir to make sure we don't pick up .craft/config somewhere in
    // the hierarchy
    let td = TempDir::new("craft").unwrap();
    assert_that!(craft_process("new").arg("foo").env("USER", "bar").env("EMAIL", "baz").cwd(td.path().clone()),
                 execs().with_status(0));

    let toml = td.path().join("foo/Craft.toml");
    let mut contents = String::new();
    File::open(&toml).unwrap().read_to_string(&mut contents).unwrap();
    assert!(contents.contains(r#"authors = ["bar <baz>"]"#));
}

#[test]
fn finds_author_git() {
    process("git").args(&["config", "--global", "user.name", "bar"]).exec().unwrap();
    process("git").args(&["config", "--global", "user.email", "baz"]).exec().unwrap();
    assert_that!(craft_process("new").arg("foo").env("USER", "foo"),
                 execs().with_status(0));

    let toml = paths::root().join("foo/Craft.toml");
    let mut contents = String::new();
    File::open(&toml).unwrap().read_to_string(&mut contents).unwrap();
    assert!(contents.contains(r#"authors = ["bar <baz>"]"#));
}

#[test]
fn finds_git_email() {
    let td = TempDir::new("craft").unwrap();
    assert_that!(craft_process("new")
                     .arg("foo")
                     .env("GIT_AUTHOR_NAME", "foo")
                     .env("GIT_AUTHOR_EMAIL", "gitfoo")
                     .cwd(td.path().clone()),
                 execs().with_status(0));

    let toml = td.path().join("foo/Craft.toml");
    let mut contents = String::new();
    File::open(&toml).unwrap().read_to_string(&mut contents).unwrap();
    assert!(contents.contains(r#"authors = ["foo <gitfoo>"]"#), contents);
}


#[test]
fn finds_git_author() {
    // Use a temp dir to make sure we don't pick up .craft/config somewhere in
    // the hierarchy
    let td = TempDir::new("craft").unwrap();
    assert_that!(craft_process("new")
                     .arg("foo")
                     .env_remove("USER")
                     .env("GIT_COMMITTER_NAME", "gitfoo")
                     .cwd(td.path().clone()),
                 execs().with_status(0));

    let toml = td.path().join("foo/Craft.toml");
    let mut contents = String::new();
    File::open(&toml).unwrap().read_to_string(&mut contents).unwrap();
    assert!(contents.contains(r#"authors = ["gitfoo"]"#));
}

#[test]
fn author_prefers_craft() {
    process("git").args(&["config", "--global", "user.name", "foo"]).exec().unwrap();
    process("git").args(&["config", "--global", "user.email", "bar"]).exec().unwrap();
    let root = paths::root();
    fs::create_dir(&root.join(".craft")).unwrap();
    File::create(&root.join(".craft/config"))
        .unwrap()
        .write_all(br#"
        [craft-new]
        name = "new-foo"
        email = "new-bar"
        vcs = "none"
    "#)
        .unwrap();

    assert_that!(craft_process("new").arg("foo").env("USER", "foo"),
                 execs().with_status(0));

    let toml = paths::root().join("foo/Craft.toml");
    let mut contents = String::new();
    File::open(&toml).unwrap().read_to_string(&mut contents).unwrap();
    assert!(contents.contains(r#"authors = ["new-foo <new-bar>"]"#));
    assert!(!root.join("foo/.gitignore").exists());
}

#[test]
fn git_prefers_command_line() {
    let root = paths::root();
    let td = TempDir::new("craft").unwrap();
    fs::create_dir(&root.join(".craft")).unwrap();
    File::create(&root.join(".craft/config"))
        .unwrap()
        .write_all(br#"
        [craft-new]
        vcs = "none"
        name = "foo"
        email = "bar"
    "#)
        .unwrap();

    assert_that!(craft_process("new").arg("foo").arg("--vcs").arg("git").cwd(td.path()).env("USER", "foo"),
                 execs().with_status(0));
    assert!(td.path().join("foo/.gitignore").exists());
}

#[test]
fn subpackage_no_git() {
    assert_that!(craft_process("new").arg("foo").env("USER", "foo"),
                 execs().with_status(0));

    let subpackage = paths::root().join("foo").join("components");
    fs::create_dir(&subpackage).unwrap();
    assert_that!(craft_process("new").arg("foo/components/subcomponent").env("USER", "foo"),
                 execs().with_status(0));

    assert_that!(&paths::root().join("foo/components/subcomponent/.git"),
                 is_not(existing_file()));
    assert_that!(&paths::root().join("foo/components/subcomponent/.gitignore"),
                 is_not(existing_file()));
}

#[test]
fn subpackage_git_with_vcs_arg() {
    assert_that!(craft_process("new").arg("foo").env("USER", "foo"),
                 execs().with_status(0));

    let subpackage = paths::root().join("foo").join("components");
    fs::create_dir(&subpackage).unwrap();
    assert_that!(craft_process("new").arg("foo/components/subcomponent").arg("--vcs").arg("git").env("USER", "foo"),
                 execs().with_status(0));

    assert_that!(&paths::root().join("foo/components/subcomponent/.git"),
                 existing_dir());
    assert_that!(&paths::root().join("foo/components/subcomponent/.gitignore"),
                 existing_file());
}

#[test]
fn unknown_flags() {
    assert_that!(craft_process("new").arg("foo").arg("--flag"),
                 execs().with_status(1).with_stderr("\
[Error] Unknown flag: '--flag'

Usage:
    craft new [..]
    craft new [..]
"));
}
