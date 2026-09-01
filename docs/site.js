(function () {
    var root = document.documentElement;
    var STORAGE_KEY = "overlingo-lang";

    function currentLang() {
        var fromUrl = new URLSearchParams(location.search).get("lang");
        if (fromUrl === "en" || fromUrl === "zh") return fromUrl;
        try {
            var saved = localStorage.getItem(STORAGE_KEY);
            if (saved === "en" || saved === "zh") return saved;
        } catch (_) {}
        var languages = navigator.languages || [navigator.language || ""];
        for (var i = 0; i < languages.length; i++) {
            if (/^zh/i.test(languages[i])) return "zh";
        }
        return "en";
    }

    function applyLang(lang) {
        root.setAttribute("data-lang", lang);
        root.setAttribute("lang", lang === "zh" ? "zh-Hans" : "en");
        document.title = lang === "zh"
            ? "OverLingo — 实时双向翻译字幕"
            : "OverLingo — Live two-way translation overlay";
        renderDownload();
    }

    // Download buttons: point at the exact asset for this platform once the release is known.
    var release = null;
    var RELEASES = "https://github.com/Deanwfy/OverLingo/releases/latest";

    function platform() {
        var ua = navigator.userAgent;
        var data = navigator.userAgentData;
        var os = data && data.platform ? data.platform : navigator.platform || "";
        if (/mac/i.test(os) || /Macintosh/.test(ua)) return "mac";
        if (/win/i.test(os) || /Windows/.test(ua)) return "win";
        return "other";
    }

    var TEXT = {
        en: { mac: "for macOS", win: "for Windows", other: "latest release", arm: "Apple Silicon" },
        zh: { mac: "macOS 版", win: "Windows 版", other: "最新版本", arm: "Apple Silicon" }
    };
    // Every asset in a release, keyed by how the menu labels it.
    var ASSETS = [
        { id: "macArm", os: "macOS", chip: "Apple Silicon", pattern: /aarch64\.dmg$/ },
        { id: "macIntel", os: "macOS", chip: "Intel", pattern: /x64\.dmg$/ },
        { id: "winX64", os: "Windows", chip: "x64", pattern: /x64-setup\.exe$/ },
        { id: "winArm", os: "Windows", chip: "ARM64", pattern: /arm64-setup\.exe$/ },
        { id: "msiX64", os: "Windows", chip: "x64", note: "MSI", pattern: /x64_.*\.msi$/ },
        { id: "msiArm", os: "Windows", chip: "ARM64", note: "MSI", pattern: /arm64_.*\.msi$/ }
    ];

    function url(item) {
        if (release) {
            for (var i = 0; i < release.assets.length; i++) {
                if (item.pattern.test(release.assets[i].name)) return release.assets[i].browser_download_url;
            }
        }
        return RELEASES;
    }

    function renderDownload() {
        var lang = root.getAttribute("data-lang");
        var t = TEXT[lang];
        var os = platform();
        var primary = document.getElementById("dl-primary");
        if (!primary) return;
        var label = document.getElementById("dl-primary-label");
        var list = document.getElementById("dl-alts");
        var version = document.getElementById("dl-version");

        version.textContent = release ? release.tag_name : "";
        var main = os === "mac" ? "macArm" : os === "win" ? "winX64" : null;
        label.textContent = os === "mac" ? t.mac + " (" + t.arm + ")" : os === "win" ? t.win + " (x64)" : t.other;

        list.textContent = "";
        ASSETS.forEach(function (item) {
            if (item.id === main) primary.href = url(item);
            if (item.id === main) return;
            var a = document.createElement("a");
            a.href = url(item);
            a.textContent = item.os + " · " + item.chip;
            if (item.note) {
                var small = document.createElement("small");
                small.textContent = item.note;
                a.appendChild(small);
            }
            list.appendChild(a);
        });
        if (!main) primary.href = RELEASES;
    }

    applyLang(currentLang());

    var toggle = document.getElementById("lang-toggle");
    if (toggle) toggle.addEventListener("click", function () {
        var next = root.getAttribute("data-lang") === "zh" ? "en" : "zh";
        try { localStorage.setItem(STORAGE_KEY, next); } catch (_) {}
        applyLang(next);
    });

    document.addEventListener("click", function (event) {
        var menu = document.querySelector(".others[open]");
        if (menu && !menu.contains(event.target)) menu.removeAttribute("open");
    });

    fetch("https://api.github.com/repos/Deanwfy/OverLingo/releases/latest", {
        headers: { Accept: "application/vnd.github+json" }
    })
        .then(function (response) { return response.ok ? response.json() : null; })
        .then(function (json) {
            if (json && json.assets) { release = json; renderDownload(); }
        })
        .catch(function () {});

    // Sections rise into place as they enter the view.
    var reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (!reduceMotion && "IntersectionObserver" in window) {
        var revealed = new IntersectionObserver(function (entries) {
            entries.forEach(function (entry) {
                if (!entry.isIntersecting) return;
                entry.target.classList.add("shown");
                revealed.unobserve(entry.target);
            });
        }, { rootMargin: "0px 0px -10% 0px" });
        document.querySelectorAll(".row, .how > h2, .how li, .facts p, .faq > h2, .faq dl > div").forEach(function (part) {
            part.classList.add("reveal");
            revealed.observe(part);
        });
    }

    var video = document.getElementById("demo");
    if (video && window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
        video.removeAttribute("autoplay");
        video.controls = true;
    }
})();
