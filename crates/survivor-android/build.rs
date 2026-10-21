//! Stamps the commit this binary was built from, as the `benilla` launcher does.

fn main() {
    benilla_buildstamp::emit();
}
