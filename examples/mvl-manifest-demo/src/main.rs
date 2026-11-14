const MANIFEST: mvl_manifest::Manifest = mvl_manifest::manifest!();

fn main() {
    // A real app would initialise its logger first (env_logger, …); without
    // one, log_manifest falls back to stderr.
    mvl_manifest::log_manifest(&MANIFEST);
    println!("{}", mvl_manifest::manifest_to_block(&MANIFEST));
}
