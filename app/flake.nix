{
  description = "Accord: a Basecamp app that reads LEZ v0.3 multisigs and decodes their approval packages";

  nixConfig = {
    extra-substituters = [ "https://cache.nix.logos.co/public" ];
    extra-trusted-public-keys = [
      "public:l4HrXgL4nw246+LBh2SOJyhz64BoGegOYLheT/iIAPU="
    ];
  };

  # The builder Logos Forum and the Basecamp 0.3.0 catalog modules are built
  # with. Its Qt is the one Basecamp 0.3.0 bundles, and from 0.3 on it also
  # cross-builds x86_64-windows.
  inputs = {
    logos-module-builder.url = "github:logos-co/logos-module-builder/0.3.1";
  };

  outputs = inputs@{ logos-module-builder, ... }:
    logos-module-builder.lib.mkLogosQmlModule {
      src = ./.;
      configFile = ./metadata.json;
      flakeInputs = inputs;
    };
}
