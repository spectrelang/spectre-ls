entry "./src/main.sx"
version "v0.0.1"

build release {
    flags "--release"
    output "./spectre-ls"
}

build dev {
    output "./spectre-ls-dev"
}
