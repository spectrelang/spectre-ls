entry "./src/main.sx"
version "v0.0.3"
spectre_version "v0.0.13"

build release {
    flags "--release"
    output "./spectre-ls"
}

build dev {
    output "./spectre-ls-dev"
}
