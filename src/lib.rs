//! This Hexchat addon provides commands that can turn on language translation
//! in any chat window of Hexhat. The user's text is translated to the target
//! language going out, and incoming message are translated back into the user's
//! own language. The user sees both the original text and translated text in
//! their Hexchat client, but other's in the channel only see the translated
//! text.
//!
//! # The addon provides the following commands
//! 
//! * `/LISTLANG` - Lists the names and 2 character codes for all the supported 
//!                 languages. The names or codes can be used to turn on 
//!                 translation with `/SETLANG`.
//! * `/SETLANG`  - Sets the source language (of the user) and the target 
//!                 language to translate to/from for the user.
//! * `/LSAY`     - Like `/SAY`, but performs translation. Required for
//!                 outgoing translations. Without using this command, the 
//!                 user's messages are sent normally. With the command they're
//!                 translated and sent to the channel.
//! * `/LME`      - A translator version of the `/ME` command.
//! * `/OFFLANG`  - Turns translation off in the current window.
//!

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::format as fm;
use std::thread;
use std::time::Duration;

use hexchat_api::*;
use StripFlags::*;
use UserData::*;

/// How long to wait for a translation to come back from the translation
/// server. The unit is seconds.
///
const TRANSLATION_SERVER_TIMEOUT: u64 = 5;

/// DeepL API endpoint for translation
const DEEPL_API_URL: &str = "https://api-free.deepl.com/v2/translate";

/// DeepL API key - should be set via environment variable DEEPL_API_KEY
/// You can get a free API key from https://www.deepl.com/pro-api
fn get_deepl_api_key() -> Option<String> {
    std::env::var("DEEPL_API_KEY").ok()
}

/// DeepL API request structure
#[derive(Serialize)]
struct DeepLRequest {
    text: Vec<String>,
    source_lang: Option<String>,
    target_lang: String,
}

/// DeepL API response structure
#[derive(Deserialize)]
struct DeepLResponse {
    translations: Vec<DeepLTranslation>,
}

#[derive(Deserialize)]
struct DeepLTranslation {
    text: String,
}

// Register the entry points of the plugin.
//
dll_entry_points!(plugin_info, plugin_init, plugin_deinit);

/// Channel data, a tuple of two strings. Used as keys in the channel map, 
/// the fields hold the `network` and `channel` strings for contexts that
/// have been enabled for translation. Used as the value in the channel map,
/// the fields hold the `source_language` and `target_language` to translate
/// between.
///
type ChanData = (String, String);

/// Maps the channels that have been activated for translation to the source
/// and target language to translate between. The keys are instances of
/// `ChanData`, as are the values.
///
type ChanMap  = HashMap<ChanData, ChanData>;

/// Called when the plugin is loaded to register it with Hexchat.
///
fn plugin_info() -> PluginInfo {
    PluginInfo::new(
        "Language Translator",
        env!("CARGO_PKG_VERSION"),
        "Instantly translated conversation in over 100 languages.")
}

/// Called when the plugin is loaded.
///
fn plugin_init(hc: &Hexchat) -> i32 {

    hc.print("Language Translator loaded");
    
    // `map_udata` holds a `HashMap` that maps contexts, `(network, channel)`, 
    // to chosen translation, `(source_lang, target_lang)`. 
    let map_udata  = UserData::shared(HashMap::<ChanData, ChanData>::new());
    
    let lsay_udata = UserData::boxed(("SAY", map_udata.clone()));
    let lme_udata  = UserData::boxed(("ME", map_udata.clone()));
    
    // Register the commands.
    
    hc.hook_command(
        "LISTLANG", Priority::Norm, on_cmd_listlang, LISTLANG_HELP, NoData);
        
    hc.hook_command(
        "SETLANG", Priority::Norm, on_cmd_setlang,   SETLANG_HELP, map_udata
                                                                   .clone());
    hc.hook_command(
        "OFFLANG", Priority::Norm, on_cmd_offlang,   OFFLANG_HELP, map_udata
                                                                   .clone());
    hc.hook_command(
        "LSAY",    Priority::Norm, on_cmd_lsay,      LSAY_HELP,    lsay_udata);

    hc.hook_command(
        "LME",     Priority::Norm, on_cmd_lsay,      LME_HELP,     lme_udata);


    // Register the handler for all the interesting text events.
    
    for event in &["Channel Message", "Channel Msg Hilight", 
                   "Channel Action",  "Channel Action Hilight", 
                   "Private Message", "Private Message to Dialog",
                   "Private Action",  "Private Action to Dialog", 
                   "You Part",        "You Part with Reason", 
                   "Disconnected"] 
    {
        let event_udata = UserData::boxed((*event, map_udata.clone()));
        
        hc.hook_print(event, Priority::Norm, on_recv_message, event_udata);
    }

    1
}

/// Called when the plugin is unloaded.
///
fn plugin_deinit(hc: &Hexchat) -> i32 {
    hc.print("Language Translator unloaded");
    1
}


/// Returns Option((sourcelang, targetlang)) for the window receiving
/// an event. If there's no entry in the map, or there's a problem accessing it,
/// `None` is returned.
/// # Arguments
/// * `hc`        - The Hexchat interface.
/// * `map_udata` - The user data of the invoking command.
/// # Returns
/// * Returns the channel data for the current context. This is obtained from
///   the `HashMap` that maps contexts to the source and dest languages.
///   If a context hasn't been set up for transation, `None` is returned.
///
fn get_channel_langs(hc        : &Hexchat, 
                     map_udata : &UserData) 
                     
    -> Option<ChanData> 
{
    let network = hc.get_info("network")?;
    let channel = hc.get_info("channel")?;
    map_udata.apply(
        |chan_map: &ChanMap| {
            chan_map.get(&(network, channel)).cloned()
        })
}

/// Activates the current context for language translation. A `HashMap` is
/// maintained that maps contexts (network/channel) to the desired translation
/// (source_lang, dest_lang).
/// # Arguments
/// * `hc`        - The Hexchat interface.
/// * `map_udata` - The user data of the invoking command.
/// * `source`    - The source language to translate from.
/// * `dest`      - The destination language to translate to.
///
fn activate(hc        : &Hexchat, 
            map_udata : &UserData, 
            source    : &str, 
            dest      : &str) 
{
    let try_activate = || {
        let network = hc.get_info("network")?;
        let channel = hc.get_info("channel")?;
        map_udata.apply_mut(
            |chan_map: &mut ChanMap| {
                chan_map.insert((network, channel), 
                                (source.to_string(), dest.to_string()));
            });
        Some(())
    };
    if try_activate().is_none() {
        hc.print(&fm!("{IRC_MAGENTA}\
                 Failed to get channel information during activation."));
    }
}

/// Removes the current context's key and value from the `HashMap` that maps
/// active contexts to translation information (source-lang, dest-lang). This
/// effectively disables language translation in that window if it was 
/// on before. It has no effect if not.
///
fn deactivate(hc        : &Hexchat, 
              map_udata : &UserData) 
{
    let try_deactivate = || {
        let network = hc.get_info("network")?;
        let channel = hc.get_info("channel")?;
        map_udata.apply_mut(
            |chan_map: &mut ChanMap| {
                chan_map.remove(&(network, channel))
            });
        Some(())
    };
    if try_deactivate().is_none() {
        hc.print(&fm!("{IRC_MAGENTA}\
                 Failed to get channel information during deactivation."));
    }
}

/// Implements the /SETLANG command. Use /SETLANG to set the source and
/// target language for translation. Issuing this command activates 
/// the channel for translation.
///
fn on_cmd_setlang(hc        : &Hexchat, 
                  word      : &[String], 
                  _word_eol : &[String], 
                  map_udata : &UserData) 
    -> Eat 
{
    if word.len() == 3 {
        let mut src_lang = word[1].as_str();
        let mut tgt_lang = word[2].as_str();
        
        let mut params_good = false;
        
        // Verify each lang is in the list below.
        if let (Some(src_lang_info), Some(tgt_lang_info))
            = (find_lang(src_lang), find_lang(tgt_lang)) {
        
            if src_lang_info !=  tgt_lang_info {
                params_good = true;
                    
                // Make sure the language names are the abbreviation.
                src_lang  =  src_lang_info.1;
                tgt_lang  =  tgt_lang_info.1;

                // Activate the channel.
                activate(hc, map_udata, src_lang, tgt_lang);
                
                hc.print(&fm!("{IRC_MAGENTA}\
                         TRANSLATION IS ON FOR THIS CHANNEL! \
                         {} (you) to {} (them).", src_lang_info.0, 
                                                  tgt_lang_info.0));
            } 
        }
        if !params_good {
            hc.print(&fm!("{IRC_MAGENTA}\
                     BAD LANGUAGE PARAMETERS. Use /LISTLANG to \
                     get a list of supported languages. And don't \
                     set translation source and target languages the \
                     same."));
        }
    } else {
        hc.print(&fm!("USAGE: {}", SETLANG_HELP));
    }
    Eat::All
}

/// Implements the /OFFLANG command. Turns translation off in the 
/// open window/channel.
///
fn on_cmd_offlang(hc        : &Hexchat, 
                  word      : &[String], 
                  _word_eol : &[String], 
                  map_udata : &UserData) 
    -> Eat 
{
    if word.len() == 1 {
        deactivate(hc, map_udata);
        hc.print(&fm!("{IRC_MAGENTA}Translation turned OFF for this channel."));
    } else {
        hc.print(&fm!("USAGE: {}", OFFLANG_HELP));
    }
    Eat::All
}

/// Implements the /LSAY and /LME commands. Use /LSAY or /LME followed 
/// by whatever text you want. The text will be translated and posted to 
/// the channel. Other users will only see the translated message.
///
fn on_cmd_lsay(hc        : &Hexchat, 
               word      : &[String], 
               word_eol  : &[String], 
               user_data : &UserData) 
    -> Eat 
{
    if let Some(eat) = try_on_cmd_lsay(hc, word, word_eol, user_data) {
        eat
    } else {
        hc.print(&fm!("{IRC_MAGENTA}\
                 Translator Error: Basic failure retrieving channel \
                 information, or unable to strip original message."));        
        Eat::All
    }
}

fn try_on_cmd_lsay(hc        : &Hexchat, 
                   _word     : &[String], 
                   word_eol  : &[String], 
                   user_data : &UserData) 
    -> Option<Eat>
{
    // Unpackage the user data to get which command this is for (LSAY/LME),
    // and get the `UserData` with the `HashMap` in it.
    let (cmd, ref map_udata) = user_data.apply(
                                    |ud: &(&str, UserData)| {
                                        (ud.0, ud.1.clone())
                                    });

    if let Some(chan_langs) = get_channel_langs(hc, map_udata) {
        let src_lang  = chan_langs.0;
        let tgt_lang  = chan_langs.1;
        let message   = word_eol[1].clone();
        
        let strip_msg = hc.strip(&message, StripBoth)?;
        let network   = hc.get_info("network")?;                              
        let channel   = hc.get_info("channel")?;

        thread::spawn(move || {
            let msg;
            let mut emsg = None;
            let mut is_over_limit = false;
            
            match deepl_translate(&strip_msg, &src_lang, &tgt_lang) {
                Ok(trans) => { 
                    msg  = trans;
                },
                Err(err)  => { 
                    msg  = err.get_partial_trans().to_string();
                    emsg = Some(fm!("{IRC_MAGENTA}{}", err));
                    is_over_limit = err.is_over_limit();
                }
            }
            if let Err(err) = main_thread(
                move |hc| -> Result<(), HexchatError> {
                    if let Some(ctx) = hc.find_context(&network, &channel) {
                        ctx.command(&fm!("{} {}", cmd, msg))?;
                        ctx.print(&fm!("{IRC_CYAN}{}", message))?;
                            
                        if let Some(emsg) = &emsg {
                            ctx.print(emsg)?;
                            if is_over_limit {
                                ctx.command("OFFLANG")?;
                            }
                        }
                    } else {
                        hc.print(&fm!("{IRC_MAGENTA}\
                                 Failed to get context."));
                    }
                    Ok(())
                }
            ).get() {
                hc_print_th!("{IRC_MAGENTA}{}", err);
            }
        });
        Some(Eat::All)
    } else {
        Some(Eat::None)
    }
}

/// Callback invoked when channel events like 'Channel Message' occur. 
/// If translation is on for the channel, this callback will have it 
/// translated and update the context window with translated message text.
///
fn on_recv_message(hc        : &Hexchat, 
                   word      : &[String], 
                   user_data : &UserData) 
    -> Eat 
{
    if let Some(eat) = try_on_recv_message(hc, word, user_data) {
        eat
    } else {
        // If we get here, either `strip()` or `get_info()` returned None.
        hc.print(&fm!("{IRC_MAGENTA}\
                 Translator Error: Basic failure retrieving channel \
                 information, or unable to strip original message."));
        Eat::Hexchat
    }
}

fn try_on_recv_message(hc        : &Hexchat, 
                       word      : &[String],
                       user_data : &UserData)
    -> Option<Eat> 
{
    if word.len() < 2 || word.last().unwrap() == "~" {
        // To avoid recursion, this handler appends the "~" to the end of
        // each `emit_print()` it generates so it can be caught here.
        return Some(Eat::None);
    }
    let (event, ref map_udata) = user_data.apply(
        |ud: &(&str, UserData)| {
            (ud.0, ud.1.clone())
        });

    if let Some(chan_langs) = get_channel_langs(hc, map_udata) {
        let sender    = word[0].clone();
        let message   = word[1].clone();
        let msg_type  = event;
        let mode_char = if word.len() > 2 
                             { word[2].clone() } 
                        else { "".to_string()  };
        let src_lang  = chan_langs.0;
        let tgt_lang  = chan_langs.1;
        
        let strip_msg = hc.strip(&message, StripBoth)?; // "throw"
        let network   = hc.get_info("network")?;
        let channel   = hc.get_info("channel")?;
        
        thread::spawn(move || {
            let msg;
            let mut emsg = None;
            let mut is_over_limit = false;
            
            match deepl_translate(&strip_msg, &tgt_lang, &src_lang) {
                Ok(trans) => { 
                    msg = trans;
                },
                Err(err)  => { 
                    msg  = err.get_partial_trans().to_string();
                    emsg = Some(fm!("{IRC_MAGENTA}{}", err));
                    is_over_limit = err.is_over_limit();
                }
            }
            if let Err(err) = main_thread(
                move |hc| -> Result<(), HexchatError> {
                    if let Some(ctx) = hc.find_context(&network, &channel) {
                        if !mode_char.is_empty() {
                            ctx.emit_print(msg_type, 
                                           &[&sender, &msg, &mode_char, "~"])?;
                        } else {
                            ctx.emit_print(msg_type, 
                                           &[&sender, &msg, "~"])?;
                        }
                        ctx.print(&fm!("{IRC_CYAN}{}", message))?;
                        if let Some(emsg) = &emsg { 
                            ctx.print(emsg)?;
                            if is_over_limit {
                                ctx.command("OFFLANG")?;
                            }
                        }
                    } else {
                        hc.print("Failed to get context.");
                    }
                    Ok(())
                }
            ).get() {
                hc_print_th!("{IRC_MAGENTA}{}", err);
            }
        });
        Some(Eat::Hexchat)
    } else {
        Some(Eat::None)
    }
}

/// Uses the DeepL API service to translate a chat text message to the 
/// desired target language.
/// # Arguments
/// * `text`    - The text to translate.
/// * `source`  - The source language of the text.
/// * `target`  - The language to translate the text to.
/// # Returns
/// * A result where `Ok()` contains the translated text, and `Err()` indicates
///   the translation failed. The error will contain an aggregate of 
///   descriptions for each problem encountered during translation.
///
fn deepl_translate(text   : &str, 
                   source : &str, 
                   target : &str)

    -> Result<String, TranslationError> 
{
    let api_key = match get_deepl_api_key() {
        Some(key) => key,
        None => {
            return Err(TranslationError::new(
                text.to_string(),
                "DeepL API key not found. Set DEEPL_API_KEY environment variable.".to_string(),
                false
            ));
        }
    };

    let agent = ureq::AgentBuilder::new()
                      .timeout_read(
                           Duration::from_secs(TRANSLATION_SERVER_TIMEOUT)
                      ).build();

    // Convert language codes to DeepL format
    let deepl_source = map_to_deepl_lang(source);
    let deepl_target = map_to_deepl_lang(target);

    let request = DeepLRequest {
        text: vec![text.to_string()],
        source_lang: if deepl_source == "auto" { None } else { Some(deepl_source.to_string()) },
        target_lang: deepl_target.to_string(),
    };

    match agent
        .post(DEEPL_API_URL)
        .set("Authorization", &format!("DeepL-Auth-Key {}", api_key))
        .set("Content-Type", "application/json")
        .send_json(&request)
    {
        Ok(response) => {
            match response.into_json::<DeepLResponse>() {
                Ok(deepl_response) => {
                    if let Some(translation) = deepl_response.translations.first() {
                        Ok(translation.text.clone())
                    } else {
                        Err(TranslationError::new(
                            text.to_string(),
                            "No translation returned from DeepL API".to_string(),
                            false
                        ))
                    }
                },
                Err(err) => {
                    Err(TranslationError::new(
                        text.to_string(),
                        format!("Failed to parse DeepL response: {}", err),
                        false
                    ))
                }
            }
        },
        Err(err) => {
            let is_over_limit = match &err {
                ureq::Error::Status(code, _) => *code == 403 || *code == 429,
                _ => false,
            };
            
            Err(TranslationError::new(
                text.to_string(),
                format!("DeepL API request failed: {}", err),
                is_over_limit
            ))
        }
    }
}

/// Maps language codes to DeepL-compatible format
fn map_to_deepl_lang(lang: &str) -> &str {
    match lang.to_lowercase().as_str() {
        "zh" => "ZH",
        "en" => "EN",
        "de" => "DE",
        "fr" => "FR",
        "it" => "IT",
        "ja" => "JA",
        "es" => "ES",
        "nl" => "NL",
        "pl" => "PL",
        "pt" => "PT",
        "ru" => "RU",
        "bg" => "BG",
        "cs" => "CS",
        "da" => "DA",
        "el" => "EL",
        "et" => "ET",
        "fi" => "FI",
        "hu" => "HU",
        "id" => "ID",
        "lv" => "LV",
        "lt" => "LT",
        "ro" => "RO",
        "sk" => "SK",
        "sl" => "SL",
        "sv" => "SV",
        "tr" => "TR",
        "uk" => "UK",
        "ar" => "AR",
        "hi" => "HI",
        "ko" => "KO",
        "nb" => "NB",
        "no" => "NB", // Map Norwegian to Norwegian Bokmål
        _ => lang, // Return as-is for unknown languages
    }
}

/// Implements the /LISTLANG command - prints out a list of all languages 
/// that the translation web services support.
///
#[allow(clippy::many_single_char_names)]     
fn on_cmd_listlang(hc        : &Hexchat, 
                   word      : &[String], 
                   _word_eol : &[String], 
                   _userdata : &UserData) 
    -> Eat 
{
    if word.len() == 1 {
        hc.print("");
        hc.print(&fm!("{IRC_CYAN}\
                  ------------------------ Supported Languages \
                  ------------------------"));
        let langs = &SUPPORTED_LANGUAGES;
        
        for i in (0..langs.len()).step_by(3) {
            let (a, b) = langs[i];
            let (c, d) = langs[i + 1];
            let (e, f) = langs[i + 2];
            hc.print(
                &fm!("{IRC_CYAN}{:-15}{:3}        {:-15}{:3}        {:-15}{:3}", 
                         a, b, c, d, e, f));
        }
        hc.print("");
    } else {
        hc.print("USAGE: ");
    }
    Eat::All
}

/// Finds and gives back a tuple (<long-name>, <abbrev>) from the supported 
/// languages list. This can be used to verify the languages the user requested
/// to see if they exist and can be used to interact with translation services.
/// # Arguments
/// * `lang` - This can be the name of the langauge, or the two character code
///            for the language.
/// # Returns
/// * If a match is found, a tuple is returned from the `SUPPORTED_LANGUAGES`
///   array. It will have the long name for the language and its two character
///   code. 
///
fn find_lang(lang: &str) -> Option<&(&str, &str)> {
    let lang = lang.to_lowercase();
    #[allow(clippy::manual_find)]
    for lang_info in &SUPPORTED_LANGUAGES {
        if lang == lang_info.0.to_lowercase() || lang == lang_info.1 {
            return Some(lang_info);
        }
    }
    None
}

/// Translation error. The error object will contain either a mix of translated
/// and untranslated messages - if some succeeded and some didn't. Or, just
/// untranslated text accessible from `get_partial_trans()`. The display
/// of the error will be an accumulated set of each unique error that occurred
/// during the translation. If the server indicated the user is over their
/// translation limit, `is_over-limit()` will reflect that.
///
#[derive(Debug)]
struct TranslationError {
    partial_trans : String,
    error_msg     : String,
    over_limit    : bool,
}

impl TranslationError {
    /// Constructs the translation error.
    /// # Arguments
    /// * `partial_trans`   - Translated and untranslated portions of the 
    ///                       original text.
    /// * `error_msg`       - The aggregate of error messages that occurred
    ///                       during the translation.
    /// * `over_limit`      - A bool indicating whether the server responded
    ///                       with a 403 error.
    ///
    fn new(partial_trans: String, error_msg: String, over_limit: bool) -> Self {
        TranslationError { partial_trans, error_msg, over_limit }
    }
    
    /// Returns the parts of translated and untranslated text - in the same
    /// order as the original text.
    ///
    fn get_partial_trans(&self) -> &str {
        &self.partial_trans
    }
    
    /// Indicates whether the translator server responded with a 403 error
    /// which means the number of translations per given span of time has 
    /// been exceeded.
    ///
    fn is_over_limit(&self) -> bool {
        self.over_limit
    }
}

impl Error for TranslationError {
    /*
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        if let Some(err) = &self.source_error {
            Some(err.as_ref())
        } else { None }
    }
    */
}

impl fmt::Display for TranslationError {

    /// Displays the aggregate of error messages that occurred during the 
    /// translation.
    ///
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Translation Error: {}", self.error_msg)
    }
}


// Help strings printed when the user requests /HELP on any of the commands 
// this addon provides.

const LISTLANG_HELP: &str = "/LISTLANG - Lists languages supported and \
                             their abbrevations. This command takes no \
                             parameters.";
                             
const SETLANG_HELP : &str = "/SETLANG <src> <tgt> - Sets source and target \
                             languages for the channel.";
                             
const OFFLANG_HELP : &str = "/OFFLANG - Deactivates translation on the \
                             channel. This command takes no paramters.";
                             
const LSAY_HELP    : &str = "/LSAY <message> - Sends a translated message \
                             to the channel.";
                             
const LME_HELP     : &str = "/LME <message> - Sends a channel action \
                             message translated.";

// A listing of all the supported langauges.

/// Supported languages by DeepL API
const SUPPORTED_LANGUAGES: [(&str, &str); 33] = [
    ("Arabic",        "ar"), ("Bulgarian",     "bg"), ("Chinese",      "zh"),
    ("Czech",         "cs"), ("Danish",        "da"), ("Dutch",        "nl"),
    ("English",       "en"), ("Estonian",      "et"), ("Finnish",      "fi"),
    ("French",        "fr"), ("German",        "de"), ("Greek",        "el"),
    ("Hungarian",     "hu"), ("Indonesian",    "id"), ("Italian",      "it"),
    ("Japanese",      "ja"), ("Korean",        "ko"), ("Latvian",      "lv"),
    ("Lithuanian",    "lt"), ("Norwegian",     "nb"), ("Polish",       "pl"),
    ("Portuguese",    "pt"), ("Romanian",      "ro"), ("Russian",      "ru"),
    ("Slovak",        "sk"), ("Slovenian",     "sl"), ("Spanish",      "es"),
    ("Swedish",       "sv"), ("Turkish",       "tr"), ("Ukrainian",    "uk"),
    ("Hindi",         "hi"), ("Arabic",        "ar"), ("",             ""  )
];		

    
