//! All regular expressions used by the engine, compiled exactly once.
//!
//! These are ports of the regexes used by mozilla/readability and
//! codelucas/newspaper (the reference implementations this engine is built on).

use regex::Regex;
use std::sync::OnceLock;

macro_rules! rx {
    ($name:ident, $pat:literal) => {
        pub fn $name() -> &'static Regex {
            static RX: OnceLock<Regex> = OnceLock::new();
            RX.get_or_init(|| Regex::new($pat).expect("static regex must compile"))
        }
    };
}

rx!(
    unlikely_candidates,
    r"(?i)-ad-|ai2html|banner|breadcrumbs|combx|comment|community|cover-wrap|disqus|extra|footer|gdpr|header|legends|menu|related|remark|replies|rss|shoutbox|sidebar|skyscraper|social|sponsor|supplemental|ad-break|agegate|pagination|pager|popup|yom-remote"
);
rx!(
    ok_maybe_its_a_candidate,
    r"(?i)and|article|body|column|content|main|mathjax|shadow"
);
rx!(
    positive,
    r"(?i)article|body|content|entry|hentry|h-entry|main|page|pagination|post|text|blog|story"
);
rx!(
    negative,
    r"(?i)-ad-|hidden|^hid$| hid$| hid |^hid |banner|combx|comment|com-|contact|footer|gdpr|masthead|media|meta|outbrain|promo|related|scroll|share|shoutbox|sidebar|skyscraper|sponsor|shopping|tags|widget"
);
rx!(byline, r"(?i)byline|author|dateline|writtenby|p-author");
rx!(normalize, r"\s{2,}");
rx!(
    videos,
    r"(?i)//(www\.)?((dailymotion|youtube|youtube-nocookie|player\.vimeo|v\.qq|bilibili|live\.bilibili)\.com|(archive|upload\.wikimedia)\.org|player\.twitch\.tv)"
);
rx!(share_elements, r"(?i)(\b|_)(share|sharedaddy)(\b|_)");
rx!(
    next_link,
    r"(?i)(next|weiter|continue|>([^\|]|$)|»([^\|]|$))"
);
rx!(prev_link, r"(?i)(prev|earl|old|new|<|«)");
rx!(whitespace, r"^\s*$");
rx!(has_content, r"\S$");
rx!(hash_url, r"^#.");
rx!(
    commas,
    r"[\u{002C}\u{060C}\u{FE50}\u{FE10}\u{FE11}\u{2E41}\u{2E34}\u{2E32}\u{FF0C}]"
);
rx!(
    ad_words,
    r"(?i)^(ad(vertising|vertisement)?|pub(licité)?|werb(ung)?|广告|Реклама|Anuncio)$"
);
rx!(
    loading_words,
    r"(?i)^((loading|正在加载|Загрузка|chargement|cargando)(…|\.\.\.)?)$"
);

rx!(title_sep_full, r"\s[|\-–—\\/>»]\s");
rx!(title_hier_spaced, r"\s[\\/>»]\s");

// readability _getArticleMetadata meta keying
rx!(
    meta_property,
    r"(?i)\s*(article|dc|dcterm|og|twitter)\s*:\s*(author|creator|description|published_time|title|site_name)\s*"
);
rx!(
    meta_name,
    r"(?i)^\s*(?:(dc|dcterm|og|twitter|parsely|weibo:(article|webpage))\s*[-\.:]\s*)?(author|creator|pub-date|description|title|site_name)\s*$"
);

rx!(
    url_date,
    r"(?x)([\./\-_]{0,1}(19|20)\d{2})[\./\-_]{0,1}(([0-3]{0,1}[0-9][\./\-_])|([A-Za-z]{3,5}[\./\-_]))([0-3]{0,1}[0-9][\./\-]{0,1})?"
);
rx!(byline_prefix, r"(?i)^(by|from|作者|记者)[:：\s]+");
rx!(name_split, r"[^\w'\-\.\p{Han}]+");
rx!(sentence_end, r"\.( |$)");

rx!(
    text_date,
    r"(?i)(Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)[a-z]*\.?\s+\d{1,2},?\s+\d{4}"
);

rx!(image_ext, r"(?i)\.(jpg|jpeg|png|webp)");
rx!(b64_data_url, r"(?i)^data:\s*([^;]*);base64,");
rx!(img_srcset_attr, r"(?i)\.(jpg|jpeg|png|webp)\s+\d");
rx!(img_src_attr, r"(?i)^\s*\S+\.(jpg|jpeg|png|webp)\S*\s*$");
rx!(srcset_url, r"(\S+)(\s+[\d.]+[xw])?(\s*(?:,|$))");

// newspaper cleaners.py regexes
rx!(
    naughty,
    r"(?i)^side$|combx|retweet|mediaarticlerelated|menucontainer|navbar|storytopbar-bucket|utility-bar|inline-share-tools|comment|PopularQuestions|contact|foot|footer|Footer|footnote|cnn_strycaptiontxt|cnn_html_slideshow|cnn_strylftcntnt|links|meta$|shoutbox|sponsor|tags|socialnetworking|socialNetworking|cnnStryHghLght|cnn_stryspcvbx|^inset$|pagetools|post-attributes|welcome_form|contentTools2|the_answers|communitypromo|runaroundLeft|subscribe|vcard|articleheadings|date|^print$|popup|author-dropdown|tools|socialtools|byline|konafilter|KonaFilter|breadcrumbs|^fn$|wp-caption-text|legende|ajoutVideo|timestamp|js_replies"
);
rx!(caption_re, r"^caption$");
rx!(google_re, r" google ");
rx!(entries_re, r"^[^entry-]more.*$");
rx!(facebook_re, r"[^-]facebook");
rx!(facebook_broadcasting_re, r"facebook-broadcasting");
rx!(twitter_re, r"[^-]twitter");
rx!(blank_line, r"(?m)^\s+$");

// mercury lead-image / next-page scoring regexes
rx!(positive_lead, r"(?i)upload|wp-content|large|photo|wp-image");
rx!(
    negative_lead,
    r"(?i)spacer|sprite|blank|throbber|gradient|tile|bg|background|icon|social|header|hdr|advert|spinner|loader|loading|default|rating|share|facebook|twitter|theme|promo|ads|wp-includes"
);
rx!(gif_re, r"(?i)\.gif(\?.*)?$");
rx!(jpg_re, r"(?i)\.jpe?g(\?.*)?$");
rx!(photo_hints, r"(?i)figure|photo|image|caption");
rx!(
    page_in_href,
    r"(?i)(page|paging|(p(a|g|ag)?(e|enum|ewanted|ing|ination)))?(=|/)([0-9]{1,3})"
);
rx!(digit, r"[0-9]");
rx!(is_digit, r"^[0-9]+$");
rx!(cap_link_text, r"(?i)(first|last|end)");
rx!(page_re, r"(?i)pag(e|ing|inat)");
rx!(
    extraneous_links,
    r"(?i)print|archive|comment|discuss|e-mail|email|share|reply|all|login|sign|single|adx|entry-unrelated"
);
rx!(
    negative_score,
    r"(?i)adbox|advert|author|bio|bookmark|bottom|byline|clear|com-|combx|comment|comment\B|contact|copy|credit|crumb|date|deck|excerpt|featured|foot|footer|footnote|graf|head|info|infotext|instapaper_ignore|jump|linebreak|link|masthead|media|meta|modal|outbrain|promo|pr_|related|respond|roundcontent|scroll|secondary|share|shopping|shoutbox|side|sidebar|sponsor|stamp|sub|summary|tags|tools|widget"
);
rx!(
    positive_score,
    r"(?i)article|articlecontent|instapaper_body|blog|body|content|entry-content-asset|entry|hentry|main|Normal|page|pagination|permalink|post|story|text|[-_]copy|\Bcopy"
);
rx!(has_alpha, r"[a-z]");
rx!(is_alpha, r"^[a-z]+$");

// readability JSON-LD
rx!(
    jsonld_article_types,
    r"^Article|AdvertiserContentArticle|NewsArticle|AnalysisNewsArticle|AskPublicNewsArticle|BackgroundNewsArticle|OpinionNewsArticle|ReportageNewsArticle|ReviewNewsArticle|Report|SatiricalArticle|ScholarlyArticle|MedicalScholarlyArticle|SocialMediaPosting|BlogPosting|LiveBlogPosting|DiscussionForumPosting|TechArticle|APIReference$"
);
rx!(schema_org, r"(?i)^https?://schema\.org/?$");

// mercury content cleaning
rx!(spacer, r"(?i)transparent|spacer|blank");
rx!(non_word, r"\W+");

// mercury value cleaners + generic extractors
rx!(domain_endings, r"(?i)\.com$|\.net$|\.org$|\.co\.uk$");
rx!(
    clean_author_re,
    r"(?i)^\s*(posted |written )?by\s*:?\s*(.*)"
);
rx!(byline_start, r"(?i)^[\s]*by");
rx!(time_meridian_dots, r"(?i)\.m\.");
rx!(ordinal_suffix, r"(?i)(\d+)(?:st|nd|rd|th)\b");
rx!(time_meridian_space, r"(?i)(.*\d)(am|pm)(.*)");
rx!(clean_date_string, r"(?i)^\s*published\s*:?\s*(.*)");
rx!(ms_date, r"^\d{13}$");
rx!(sec_date, r"^\d{10}$");
rx!(
    time_ago,
    r"(?i)(\d+)\s+(seconds?|minutes?|hours?|days?|weeks?|months?|years?)\s+ago"
);
rx!(time_now, r"(?i)^\s*(just|right)?\s*now\s*");
rx!(
    split_date_string,
    r"(?i)[0-9]{1,2}:[0-9]{2}( ?[ap]\.?m\.?)?|[0-9]{1,2}[/-][0-9]{1,2}[/-][0-9]{2,4}|-[0-9]{3,4}$|[0-9]{1,4}|jan|feb|mar|apr|may|jun|jul|aug|sep|oct|nov|dec"
);
rx!(url_yyyymmdd_slash, r"(?i)/(20\d{2}/\d{2}/\d{2})/");
rx!(url_yyyymmdd_dash, r"(?i)(20\d{2}-[01]\d-[0-3]\d)");
rx!(
    url_yyyymm_mon,
    r"(?i)/(20\d{2}/(jan|feb|mar|apr|may|jun|jul|aug|sep|oct|nov|dec)/[0-3]\d)/"
);
rx!(pre_close, r"[^<>]*</(pre|code|textarea)>");
rx!(
    abs_url,
    r"(?i)^(?:http|ftp)s?://(?:(?:[A-Z0-9](?:[A-Z0-9-]{0,61}[A-Z0-9])?\.)+(?:[A-Z]{2,6}\.?|[A-Z0-9-]{2,}\.?)|localhost|\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}|\[?[A-F0-9]*:[A-F0-9:]+\]?)(?::\d+)?(?:/?|[/?]\S+)$"
);
rx!(embedly_yt, r"https://i\.ytimg\.com/vi/(\w+)/");

// HTML meta-charset sniffing: <meta charset=...> and
// <meta http-equiv=content-type content="text/html; charset=...">.
rx!(
    meta_charset,
    r#"<meta[^>]+charset\s*=\s*["']?([a-zA-Z0-9_-]+)"#
);

// mercury resource-level lazy image conversion
rx!(is_link, r"(?i)https?://");
rx!(is_image, r"(?i)\.(png|gif|jpe?g)");
rx!(is_srcset, r"(?i)\.(png|gif|jpe?g)(\?\S+)?(\s*[\d.]+[wx])");
