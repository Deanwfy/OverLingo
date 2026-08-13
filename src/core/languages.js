const COMMON = [
    ['en', 'English', '英语', 'Tiếng Anh'],
    ['zh', 'Chinese', '中文', 'Tiếng Trung'],
    ['vi', 'Vietnamese', '越南语', 'Tiếng Việt'],
    ['ja', 'Japanese', '日语', 'Tiếng Nhật'],
    ['ko', 'Korean', '韩语', 'Tiếng Hàn'],
    ['es', 'Spanish', '西班牙语', 'Tiếng Tây Ban Nha'],
    ['fr', 'French', '法语', 'Tiếng Pháp'],
    ['de', 'German', '德语', 'Tiếng Đức'],
    ['ru', 'Russian', '俄语', 'Tiếng Nga'],
    ['pt', 'Portuguese', '葡萄牙语', 'Tiếng Bồ Đào Nha'],
    ['it', 'Italian', '意大利语', 'Tiếng Ý'],
    ['id', 'Indonesian', '印度尼西亚语', 'Tiếng Indonesia'],
    ['th', 'Thai', '泰语', 'Tiếng Thái'],
    ['hi', 'Hindi', '印地语', 'Tiếng Hindi'],
    ['ar', 'Arabic', '阿拉伯语', 'Tiếng Ả Rập'],
    ['tr', 'Turkish', '土耳其语', 'Tiếng Thổ Nhĩ Kỳ'],
    ['yue', 'Cantonese', '粤语', 'Tiếng Quảng Đông'],
];

const EXTENDED = [
    ['af', 'Afrikaans'], ['ast', 'Asturian'], ['az', 'Azerbaijani'],
    ['be', 'Belarusian'], ['bg', 'Bulgarian'], ['bn', 'Bengali'],
    ['bs', 'Bosnian'], ['ca', 'Catalan'], ['ceb', 'Cebuano'],
    ['cs', 'Czech'], ['da', 'Danish'], ['el', 'Greek'],
    ['et', 'Estonian'], ['fa', 'Persian'], ['fi', 'Finnish'],
    ['fil', 'Filipino'], ['gl', 'Galician'], ['gu', 'Gujarati'],
    ['he', 'Hebrew'], ['hr', 'Croatian'], ['hu', 'Hungarian'],
    ['is', 'Icelandic'], ['jv', 'Javanese'], ['kk', 'Kazakh'],
    ['kn', 'Kannada'], ['ky', 'Kyrgyz'], ['lv', 'Latvian'],
    ['mk', 'Macedonian'], ['ml', 'Malayalam'], ['mr', 'Marathi'],
    ['ms', 'Malay'], ['nb', 'Norwegian Bokmål'], ['nl', 'Dutch'],
    ['pa', 'Punjabi'], ['pl', 'Polish'], ['ro', 'Romanian'],
    ['sk', 'Slovak'], ['sl', 'Slovenian'], ['sv', 'Swedish'],
    ['sw', 'Swahili'], ['tg', 'Tajik'], ['uk', 'Ukrainian'],
    ['ur', 'Urdu'],
];

export const LANGUAGES = [...COMMON, ...EXTENDED].map(([code, en, zh = en, vi = en]) => ({
    code,
    labels: { en, 'zh-Hans': zh, vi },
}));

export function languageName(code, locale = 'en') {
    try {
        const name = new Intl.DisplayNames([locale], { type: 'language' }).of(code);
        if (name && name !== code) return name;
    } catch {}
    const language = LANGUAGES.find(item => item.code === code);
    return language?.labels[locale] || language?.labels.en || code;
}

export function nativeLanguageName(code) {
    const nativeLocale = code === 'zh' ? 'zh-Hans' : code === 'yue' ? 'zh-Hant-HK' : code;
    return languageName(code, nativeLocale);
}

export function translationDirection(source, target, fallbackLocale = 'en') {
    const locale = target === 'zh'
        ? 'zh-Hans'
        : ['en', 'es', 'ja', 'ko', 'vi'].includes(target)
            ? target
            : fallbackLocale;
    return `${languageName(source, locale)} → ${languageName(target, locale)}`;
}
