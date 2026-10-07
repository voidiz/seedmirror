{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = {
    nixpkgs,
    rust-overlay,
    ...
  }: let
    system = "x86_64-linux";
    pkgs = import nixpkgs {
      inherit system;
      overlays = [rust-overlay.overlays.default];
    };

    rustToolchain = (pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml)
      .override {
      extensions = ["clippy" "rustfmt"];
      targets = ["x86_64-unknown-linux-musl"];
    };

    muslCC = pkgs.pkgsMusl.stdenv.cc;
  in {
    devShells.${system}.default = pkgs.mkShell {
      packages = [
        rustToolchain
        pkgs.pnpm
        pkgs.nodejs
      ];

      CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER = "${muslCC}/bin/cc";
      CC_x86_64_unknown_linux_musl = "${muslCC}/bin/cc";
      CXX_x86_64_unknown_linux_musl = "${muslCC}/bin/c++";
    };
  };
}
