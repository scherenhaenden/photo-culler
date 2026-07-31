"""Small, dependency-free internationalisation layer for the web interface."""

from __future__ import annotations

import re
from dataclasses import dataclass
from html import escape
from typing import Mapping

from fastapi import Request


@dataclass(frozen=True)
class Language:
    code: str
    name: str
    native_name: str


LANGUAGES = (
    Language("es", "Spanish", "Español"),
    Language("en", "English", "English"),
    Language("de", "German", "Deutsch"),
    Language("it", "Italian", "Italiano"),
    Language("pt", "Portuguese", "Português"),
    Language("fr", "French", "Français"),
    Language("nl", "Dutch", "Nederlands"),
    Language("pl", "Polish", "Polski"),
    Language("cs", "Czech", "Čeština"),
    Language("da", "Danish", "Dansk"),
    Language("sv", "Swedish", "Svenska"),
    Language("no", "Norwegian", "Norsk"),
    Language("fi", "Finnish", "Suomi"),
    Language("el", "Greek", "Ελληνικά"),
    Language("tr", "Turkish", "Türkçe"),
    Language("ru", "Russian", "Русский"),
    Language("uk", "Ukrainian", "Українська"),
    Language("ja", "Japanese", "日本語"),
    Language("ko", "Korean", "한국어"),
    Language("zh", "Chinese", "简体中文"),
)
SUPPORTED_LOCALES = frozenset(language.code for language in LANGUAGES)

# The source UI historically mixed Spanish and English.  Keeping source strings as
# message ids lets the application migrate template-by-template without untranslated
# screens.  Entries absent from a locale intentionally use the Spanish source text.
_CORE_TERMS: Mapping[str, Mapping[str, str]] = {
    "en": {
        "Biblioteca": "Library",
        "Análisis": "Analysis",
        "Grupos": "Groups",
        "Sesiones": "Sessions",
        "Dashboard": "Dashboard",
        "Dashboard del Catálogo": "Catalog Dashboard",
        "Biblioteca de Fotografías": "Photo Library",
        "Importar galería": "Import gallery",
        "Perfiles de análisis": "Analysis profiles",
        "Sesiones y Ráfagas": "Sessions and Bursts",
        "Selection Stats": "Selection stats",
        "Kept": "Kept",
        "Rejected": "Rejected",
        "Unrated": "Unrated",
    },
    "de": {
        "Biblioteca": "Bibliothek",
        "Análisis": "Analyse",
        "Grupos": "Gruppen",
        "Sesiones": "Sitzungen",
        "Dashboard": "Übersicht",
        "Dashboard del Catálogo": "Katalogübersicht",
        "Biblioteca de Fotografías": "Fotobibliothek",
        "Importar galería": "Galerie importieren",
        "Perfiles de análisis": "Analyseprofile",
        "Sesiones y Ráfagas": "Sitzungen und Serien",
        "Selection Stats": "Auswahlstatistik",
        "Kept": "Behalten",
        "Rejected": "Abgelehnt",
        "Unrated": "Unbewertet",
    },
    "it": {
        "Biblioteca": "Libreria",
        "Análisis": "Analisi",
        "Grupos": "Gruppi",
        "Sesiones": "Sessioni",
        "Dashboard": "Pannello",
        "Dashboard del Catálogo": "Pannello del catalogo",
        "Biblioteca de Fotografías": "Libreria fotografica",
        "Importar galería": "Importa galleria",
        "Perfiles de análisis": "Profili di analisi",
        "Sesiones y Ráfagas": "Sessioni e raffiche",
        "Selection Stats": "Statistiche selezione",
        "Kept": "Conservate",
        "Rejected": "Rifiutate",
        "Unrated": "Non valutate",
    },
    "pt": {
        "Biblioteca": "Biblioteca",
        "Análisis": "Análise",
        "Grupos": "Grupos",
        "Sesiones": "Sessões",
        "Dashboard": "Painel",
        "Dashboard del Catálogo": "Painel do catálogo",
        "Biblioteca de Fotografías": "Biblioteca de fotografias",
        "Importar galería": "Importar galeria",
        "Perfiles de análisis": "Perfis de análise",
        "Sesiones y Ráfagas": "Sessões e rajadas",
        "Selection Stats": "Estatísticas da seleção",
        "Kept": "Mantidas",
        "Rejected": "Rejeitadas",
        "Unrated": "Sem avaliação",
    },
}

# Compact navigation translations for the other supported languages.
_NAV_VALUES = {
    "fr": ("Photothèque", "Analyse", "Groupes", "Sessions"),
    "nl": ("Bibliotheek", "Analyse", "Groepen", "Sessies"),
    "pl": ("Biblioteka", "Analiza", "Grupy", "Sesje"),
    "cs": ("Knihovna", "Analýza", "Skupiny", "Relace"),
    "da": ("Bibliotek", "Analyse", "Grupper", "Sessioner"),
    "sv": ("Bibliotek", "Analys", "Grupper", "Sessioner"),
    "no": ("Bibliotek", "Analyse", "Grupper", "Økter"),
    "fi": ("Kirjasto", "Analyysi", "Ryhmät", "Istunnot"),
    "el": ("Βιβλιοθήκη", "Ανάλυση", "Ομάδες", "Συνεδρίες"),
    "tr": ("Kitaplık", "Analiz", "Gruplar", "Oturumlar"),
    "ru": ("Библиотека", "Анализ", "Группы", "Сеансы"),
    "uk": ("Бібліотека", "Аналіз", "Групи", "Сеанси"),
    "ja": ("ライブラリ", "分析", "グループ", "セッション"),
    "ko": ("라이브러리", "분석", "그룹", "세션"),
    "zh": ("照片库", "分析", "分组", "会话"),
}
_NAV_KEYS = ("Biblioteca", "Análisis", "Grupos", "Sesiones")
_NAV_TERMS = {locale: dict(zip(_NAV_KEYS, values)) for locale, values in _NAV_VALUES.items()}
_TERMS: Mapping[str, Mapping[str, str]] = {**_CORE_TERMS, **_NAV_TERMS}


def resolve_locale(request: Request) -> str:
    """Resolve query, cookie, then browser preference, in that order."""
    requested = request.query_params.get("lang")
    if requested in SUPPORTED_LOCALES:
        return requested
    saved = request.cookies.get("photo_culler_locale")
    if saved in SUPPORTED_LOCALES:
        return saved
    for item in request.headers.get("accept-language", "").split(","):
        code = item.split(";", 1)[0].strip().lower().split("-", 1)[0]
        if code in SUPPORTED_LOCALES:
            return code
    return "es"


def translate(message: str, locale: str) -> str:
    return _TERMS.get(locale, {}).get(message, message)


def language_selector(locale: str) -> str:
    options = "".join(
        f'<option value="{lang.code}"{" selected" if lang.code == locale else ""}>{escape(lang.native_name)}</option>'
        for lang in LANGUAGES
    )
    return f'<label class="language-picker"><span>🌐</span><select id="language-picker" aria-label="Language">{options}</select></label>'


_TEXT_NODE = re.compile(r">([^<>]+)<")
_HTML_LANG = re.compile(r'(<html\b[^>]*\blang\s*=\s*["\'])[^"\']*(["\'])', re.IGNORECASE)


def localize_html(document: str, locale: str) -> str:
    """Translate exact text nodes while leaving data, markup and scripts intact."""
    document = _HTML_LANG.sub(rf"\g<1>{locale}\g<2>", document, count=1)

    def replace(match: re.Match[str]) -> str:
        raw = match.group(1)
        stripped = raw.strip()
        if not stripped:
            return match.group(0)
        translated = translate(stripped, locale)
        return ">" + raw.replace(stripped, translated, 1) + "<"

    # Script/style contents contain comparison operators, so only translate the
    # rendered markup before the first script. Dynamic DOM is handled by i18n.js.
    head, separator, tail = document.partition("<script")
    return _TEXT_NODE.sub(replace, head) + (separator + tail if separator else "")
