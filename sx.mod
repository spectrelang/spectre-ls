entry "./src/main.sx"
version "v0.0.4"
spectre_version "v0.0.14"

build release {
    flags "-release"
    output "./spectre-ls"
}

build dev {
    output "./spectre-ls-dev"
}
