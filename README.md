[![Rust](https://github.com/SomewhatAwake/hexchat_translator/actions/workflows/rust.yml/badge.svg)](https://github.com/SomewhatAwake/hexchat_translator/actions/workflows/rust.yml)
# Hexchat Translator

This is a plugin to Hexchat that provides translated chat features enabling one 
to easily chat with people in other tongues. Translation is automatic so all
participants can focus on the conversation without any thought to the
translations taking place.

Your outgoing messages will be translated into the target language of your
choice, and incoming messages will be translated back into your native tongue.
The translated text will be on the first line, with the original message
below it.

The plugin was implemented in Rust using a 
[hexchat-api](https://crates.io/crates/hexchat-api)
to the Hexchat Plugin Interface. The translations are provided by DeepL's
API service. DeepL provides high-quality translations with a free tier
that includes 500,000 characters per month.

## Setup

To use this plugin, you need to obtain a free DeepL API key:

1. Visit [DeepL API Free](https://www.deepl.com/pro-api) and sign up for a free account
2. Get your API authentication key from the account settings
3. Set the environment variable `DEEPL_API_KEY` to your API key before starting Hexchat

### Setting the Environment Variable

**Windows:**
```cmd
setx DEEPL_API_KEY "your-deepl-api-key-here"
```

**Linux/macOS:**
```bash
export DEEPL_API_KEY="your-deepl-api-key-here"
```

You can also add the export line to your shell profile (`.bashrc`, `.zshrc`, etc.) to make it permanent.

## Hexchat Commands
* `/LISTLANG` 
    * Lists all the supported langauges.
* `/SETLANG <your-language> <other-langauge>`
    * Sets the the languages to translate to/from in the current channel.
* `/LSAY <message>`
    * Like `/SAY`, sends a translated message to the IRC chat channel.
* `/LME <emote-message>`
    * Like `/ME`, sends a translated emote message to the channel.
* `/OFFLANG`
    * Turns off translation in the current channel.

The help for these 
can be accessed through the Hexchat "/HELP" command.

This plugin is stable, but experimental. It interacts with DeepL's 
translation API service which provides high-quality translations with generous 
rate limits on the free tier. 

To add it to Hexchat, you can put the relevant binary in the "addons" 
folder of your system's Hexchat config directory.
* `~/.config/hexchat/addons` for Linux
* `%APPDATA%\HexChat\addons` on Windows

Or you can load it directly from the UI: 
* `Window > Plugins and Scripts > Load` - then navigate to the file and load it.

## Building
It's fairly easy to set up a Rust build environment on your system. You can find
instructions [here](https://www.rust-lang.org/learn/get-started). The process
is automated using `rustup`. Once that's in place, simply clone this project 
and launch the build process:
* `git clone https://github.com/ttappr/hexchat_translator.git`
* `cd hexchat_translator`
* `cargo build --release`
* `cd target/release && ls -al` and there's your binary.

## Rust Hexchat API
This project uses a 
[Rust Hexchat API lib](https://crates.io/crates/hexchat-api), 
which other developers may find useful for writing their own Rust Hexchat 
plugins. It has some nice features like
* A thread-safe API.
* Simple `user_data` objects.
* Abstractions like `Context` that make it simple to interact with specific 
  tabs/windows in the UI.
* Panic's are caught and displayed in the active window.


