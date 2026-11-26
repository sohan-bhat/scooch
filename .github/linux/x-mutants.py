import subprocess

gnome = "crates/scooch/src/platform/linux/gnome.rs"
linux = "crates/scooch/src/platform/linux/mod.rs"
read_test = "platform::linux::gnome::tests::a_read_of_the_picture_that_got_no_answer_stays_an_error"
kinds_test = "platform::linux::tests::only_gnome_loads_the_picture_again_when_its_file_changes"
mutations = [
    ("the GNOME read swallows a read that got no answer", gnome, "Ok(read?.and_then(", "Ok(read.ok().flatten().and_then(", read_test),
    ("GNOME says its shell does not load the file again", gnome, "    fn reloads_picture(&self) -> bool {\n        true\n    }", "    fn reloads_picture(&self) -> bool {\n        false\n    }", kinds_test),
    ("every Linux shell says it loads the file again", linux, "        fn reloads_picture(&self) -> bool {\n            false\n        }\n        fn per_screen", "        fn reloads_picture(&self) -> bool {\n            true\n        }\n        fn per_screen", kinds_test),
    ("the Linux desktop answers without asking its shell", linux, "            self.shell.reloads_picture()", "            false", kinds_test),
]
subprocess.run(["cargo", "test", "--release", "-p", "scooch", "--lib", "--no-run"], capture_output=True)
before = subprocess.run(["cargo", "test", "--release", "-p", "scooch", "--lib", "--", "--exact", read_test, kinds_test], capture_output=True, text=True)
print("== before any mutation", [line for line in before.stdout.splitlines() if line.startswith("test ")], flush=True)
for name, path, old, new, test in mutations:
    text = open(path).read()
    if text.count(old) != 1:
        print("== %s: the mutation did not apply, %d matches" % (name, text.count(old)), flush=True)
        continue
    open(path, "w").write(text.replace(old, new))
    try:
        run = subprocess.run(["cargo", "test", "--release", "-p", "scooch", "--lib", "--", "--exact", test], capture_output=True, text=True)
        failed = [line for line in run.stdout.splitlines() if line.startswith("test ") and line.endswith("FAILED")]
        print("== %s: %s" % (name, ("caught by " + ", ".join(failed)) if failed else ("did not build" if "could not compile" in run.stderr else "NOT CAUGHT")), flush=True)
    finally:
        open(path, "w").write(text)
