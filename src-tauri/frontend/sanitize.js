// Michael Burgus — https://github.com/NeuralDrifter
// Strips anything executable from book markup before foliate renders it.
// foliate's section iframes are sandboxed with allow-same-origin + allow-scripts,
// so a script in a book could otherwise reach window.parent.__TAURI__.

const MARKUP_TYPES = new Set(['application/xhtml+xml', 'text/html', 'image/svg+xml'])

const BLOCKED_ELEMENTS = 'script, iframe, frame, frameset, object, embed, applet, base, meta[http-equiv], form'
const URL_ATTRS = ['href', 'src', 'action', 'formaction', 'data', 'xlink:href']
const DANGEROUS_URL = /^\s*(javascript|vbscript|data\s*:\s*text\/html)/i

export const isMarkup = type => MARKUP_TYPES.has(type)

export function sanitizeMarkup(text, type) {
    const doc = new DOMParser().parseFromString(text, type)
    if (doc.querySelector('parsererror')) return stripScriptsFallback(text)

    for (const el of doc.querySelectorAll(BLOCKED_ELEMENTS)) el.remove()

    for (const el of doc.querySelectorAll('*')) {
        for (const attr of [...el.attributes]) {
            const name = attr.name.toLowerCase()
            if (name.startsWith('on')) el.removeAttributeNode(attr)
            else if (URL_ATTRS.includes(name) && DANGEROUS_URL.test(attr.value))
                el.removeAttributeNode(attr)
        }
    }

    return type === 'text/html'
        ? '<!DOCTYPE html>\n' + doc.documentElement.outerHTML
        : new XMLSerializer().serializeToString(doc)
}

// Used only when the document doesn't parse; foliate would fail on it anyway,
// but never hand back markup that could still contain a script.
function stripScriptsFallback(text) {
    return text
        .replace(/<script[\s\S]*?<\/script\s*>/gi, '')
        .replace(/<script[^>]*\/>/gi, '')
        .replace(/\son[a-z]+\s*=\s*("[^"]*"|'[^']*'|[^\s>]+)/gi, '')
        .replace(/javascript:/gi, '')
}

/** Attach to a foliate book so every loaded document is sanitized. */
export function attachSanitizer(book) {
    book.transformTarget?.addEventListener('data', ({ detail }) => {
        if (!isMarkup(detail.type)) return
        detail.data = Promise.resolve(detail.data).then(data =>
            typeof data === 'string' ? sanitizeMarkup(data, detail.type) : data)
    })
}
