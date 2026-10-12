{
  description = "Accord: a Basecamp app that reads LEZ v0.3 multisigs, decodes their approval packages, and carries them over a Logos Chat room";

  nixConfig = {
    extra-substituters = [ "https://cache.nix.logos.co/public" ];
    extra-trusted-public-keys = [ "public:l4HrXgL4nw246+LBh2SOJyhz64BoGegOYLheT/iIAPU=" ];
  };

  # The room is a Logos Chat group conversation: build against the chat module
  # release the package manager resolves, and its own builder and delivery pins.
  inputs = {
    chat_module.url = "github:logos-co/logos-chat-module/v0.3.0";
    logos-module-builder.follows = "chat_module/logos-module-builder";
    logos-delivery-module.follows = "chat_module/logos-delivery-module";
  };

  outputs = inputs@{ logos-module-builder, logos-delivery-module, ... }:
    logos-module-builder.lib.mkLogosQmlModule {
      src = ./.;
      configFile = ./metadata.json;
      flakeInputs = { delivery_module = logos-delivery-module; } // inputs;
    };
}
