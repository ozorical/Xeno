const $ = (s, el = document) => el.querySelector(s)
const $$ = (s, el = document) => [...el.querySelectorAll(s)]
const send = m => chrome.webview.postMessage(m)
const esc = s => String(s ?? '').replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`)

const paths = {
  search: '<circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/>',
  globe: '<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18"/>',
  clock: '<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/>',
  go: '<path d="M7 17 17 7M8 7h9v9"/>',
  bolt: '<path d="M13 3 5 14h6l-1 7 8-11h-6z"/>',
  moon: '<path d="M20 14.5A8 8 0 1 1 9.5 4a6.5 6.5 0 0 0 10.5 10.5z"/>',
  sound: '<path d="M11 5 6 9H3v6h3l5 4z"/><path d="M15.5 8.5a5 5 0 0 1 0 7M18.5 5.5a9 9 0 0 1 0 13"/>',
  muted: '<path d="M11 5 6 9H3v6h3l5 4z"/><path d="m16 9 5 6M21 9l-5 6"/>',
  x: '<path d="M6 6l12 12M18 6 6 18"/>',
  sliders: '<path d="M4 7h10M18 7h2M4 17h4M12 17h8"/><circle cx="16" cy="7" r="2"/><circle cx="10" cy="17" r="2"/>',
  palette: '<path d="M12 3a9 9 0 1 0 0 18c1.1 0 1.6-.9 1.2-1.8l-.4-.9c-.5-1 .2-2.3 1.4-2.3H17a4 4 0 0 0 4-4c0-5-4-9-9-9z"/><circle cx="7.5" cy="11" r="1"/><circle cx="10.5" cy="7" r="1"/><circle cx="15.5" cy="7.5" r="1"/>',
  gauge: '<path d="M4.5 17a8.5 8.5 0 1 1 15 0"/><path d="m12 13 3.5-4"/>',
  shield: '<path d="M12 3 5 6v5c0 4.5 3 8 7 10 4-2 7-5.5 7-10V6z"/>',
  puzzle: '<path d="M9.5 4.5a2 2 0 1 1 4 0V6H17a1 1 0 0 1 1 1v3.5h-1.5a2 2 0 1 0 0 4H18V18a1 1 0 0 1-1 1h-3.5v-1.5a2 2 0 1 0-4 0V19H6a1 1 0 0 1-1-1v-3.5h1.5a2 2 0 1 0 0-4H5V7a1 1 0 0 1 1-1h3.5z"/>',
  keyboard: '<rect x="3" y="6" width="18" height="12" rx="2.5"/><path d="M7 10h.01M11 10h.01M15 10h.01M7 14h10"/>',
  info: '<circle cx="12" cy="12" r="9"/><path d="M12 11v5M12 8h.01"/>',
}

const icon = name => `<svg viewBox="0 0 24 24">${paths[name]}</svg>`
const fav = (src, fallback = 'globe') => (src ? `<img src="${esc(src)}" data-fallback="${fallback}" alt="">` : icon(fallback))
const host = url => {
  try {
    return new URL(url).host.replace(/^www\./, '') || url
  } catch {
    return url
  }
}

let config = {}
let meta = {}
let boot = null
let tabs = []
let active = null
let mru = []
let mode = 'none'
let exts = []
let usage = null
const on = {}

chrome.webview.addEventListener('message', e => on[e.data.t]?.(e.data))
document.addEventListener('error', e => {
  if (e.target.dataset?.fallback) e.target.outerHTML = icon(e.target.dataset.fallback)
}, true)

const keysOf = action => (config.keys?.[action] || '').split(',').map(s => s.trim()).filter(Boolean)
const kbd = combo => combo.split('+').map(k => `<kbd>${esc(k)}</kbd>`).join('')

function glide(list, index) {
  const hl = $('.hl', list)
  const rows = $$('.row', list)
  rows.forEach((r, i) => r.classList.toggle('on', i === index))
  const row = rows[index]
  if (!row) return (hl.style.opacity = 0)
  hl.style.opacity = 1
  hl.style.height = `${row.offsetHeight}px`
  hl.style.transform = `translateY(${row.offsetTop}px)`
  row.scrollIntoView({ block: 'nearest' })
}

function replay(el) {
  el.classList.remove('enter')
  void el.offsetWidth
  el.classList.add('enter')
  clearTimeout(el.settle)
  el.settle = setTimeout(() => el.classList.remove('enter'), 900)
}

const bar = $('#bar')
const input = $('#q')
const ghost = $('#ghost')
const results = $('#results')
let items = []
let sel = 0
let fresh = false
let home = false
let found = []
let hints = []
let complete = null
let erasing = false
let untouched = false
let typing

const commands = [
  ['New tab', 'new'],
  ['Close tab', 'close', true],
  ['Reopen closed tab', 'reopen'],
  ['Reload page', 'reload', true],
  ['Go back', 'back', true],
  ['Go forward', 'forward', true],
  ['Copy link', 'copy', true],
  ['Switch tabs', 'switch', true],
  ['Settings', 'settings'],
  ['Extensions', 'extensions'],
  ['Zoom in', 'zoom-in', true],
  ['Zoom out', 'zoom-out', true],
  ['Reset zoom', 'zoom-reset', true],
  ['Print', 'print', true],
  ['Developer tools', 'devtools', true],
  ['Mute tab', 'mute', true],
  ['Sleep background tabs', 'sleep', true],
  ['Open downloads', 'downloads'],
  ['Clear history', 'clear-history'],
  ['Clear browsing data', 'clear-data'],
  ['Restart Xeno', 'restart'],
  ['Quit Xeno', 'quit'],
]

function looksLikeUrl(s) {
  if (!s || /\s/.test(s)) return false
  if (/^[a-z][a-z0-9+.-]*:(?!\d)/i.test(s)) return true
  const name = s.split(/[/?#]/)[0].replace(/:\d+$/, '')
  return name === 'localhost' || /^(\d{1,3}\.){3}\d{1,3}$/.test(name) || /^([a-z0-9-]+\.)+[a-z]{2,}$/i.test(name)
}

function engineName() {
  return meta.engines?.find(e => e.id === config.engine)?.name || 'the web'
}

function completion() {
  const v = input.value
  if (!complete || erasing || !v || /\s/.test(v) || input.selectionEnd !== v.length) return null
  return complete.toLowerCase().startsWith(v.toLowerCase()) && complete.length > v.length ? complete : null
}

function build() {
  const q = input.value.trim()
  const lower = q.toLowerCase()
  const out = []
  const add = (kind, title, sub, glyph, hint, run) => out.push({ kind, title, sub, glyph, hint, run })
  const tab = t => add('tab', t.title || 'New tab', host(t.url), fav(t.icon), 'Switch', () => send({ t: 'activate', id: t.id }))
  const cmd = ([name, id]) => add('cmd', name, '', icon('bolt'), 'Run', () => send({ t: 'cmd', name: id }))

  if (!q || untouched) {
    mru.filter(id => id !== active).map(id => tabs.find(t => t.id === id)).filter(Boolean).slice(0, 6).forEach(tab)
    return out
  }

  const full = completion() || q
  if (looksLikeUrl(full)) add('go', full, 'Open', icon('go'), 'Open', n => send({ t: 'go', text: full, tab: n }))
  else add('search', q, `Search ${engineName()}`, icon('search'), 'Search', n => send({ t: 'search', text: q, tab: n }))

  const words = lower.split(/\s+/)
  const hit = s => words.every(w => s.toLowerCase().includes(w))
  const usable = commands.filter(c => !c[2] || tabs.length)
  const leading = usable.filter(c => c[0].toLowerCase().startsWith(lower))
  const open = new Set(tabs.map(t => t.url))

  leading.slice(0, 2).forEach(cmd)
  tabs.filter(t => t.id !== active && hit(`${t.title} ${t.url}`)).slice(0, 4).forEach(tab)
  found
    .filter(h => !open.has(h.url) && hit(`${h.title} ${h.url}`))
    .slice(0, 5)
    .forEach(h => add('history', h.title || host(h.url), h.url, icon('clock'), 'Open', n => send({ t: 'open', url: h.url, tab: n })))
  hints
    .filter(s => s.toLowerCase() !== lower)
    .slice(0, 5)
    .forEach(s => add('suggest', s, '', icon('search'), 'Search', n => send({ t: 'search', text: s, tab: n })))
  usable.filter(c => !leading.includes(c) && hit(c[0])).slice(0, 3).forEach(cmd)
  return out
}

function render() {
  items = build()
  sel = Math.max(untouched ? -1 : 0, Math.min(sel, items.length - 1))
  $('.rows', results).innerHTML = items
    .map(
      (it, i) =>
        `<div class="row" style="--i:${i}" data-i="${i}"><span class="icon">${it.glyph}</span><div class="text"><div class="title">${esc(it.title)}</div>${it.sub ? `<div class="sub">${esc(it.sub)}</div>` : ''}</div><span class="hint">${esc(it.hint)}</span></div>`
    )
    .join('')
  $('#glyph').innerHTML = icon(items[0]?.kind === 'go' ? 'go' : 'search')
  const c = completion()
  ghost.innerHTML = c && input.scrollWidth <= input.clientWidth ? `<span>${esc(input.value)}</span>${esc(c.slice(input.value.length))}` : ''
  glide(results, sel)
}

function run(i, newTab) {
  items[i]?.run(fresh || home || newTab)
}

function openBar(m) {
  fresh = m.fresh
  home = m.home
  found = []
  hints = []
  complete = null
  sel = 0
  input.value = fresh || home ? '' : m.url
  untouched = !!input.value
  sel = untouched ? -1 : 0
  replay(results)
  render()
  requestAnimationFrame(() => {
    input.focus()
    input.setSelectionRange(0, input.value.length, 'backward')
    input.scrollLeft = 0
  })
}

input.addEventListener('input', e => {
  erasing = e.inputType?.startsWith('delete') ?? false
  untouched = false
  sel = 0
  const q = input.value.trim()
  clearTimeout(typing)
  if (q) typing = setTimeout(() => send({ t: 'query', q }), 40)
  else {
    found = []
    hints = []
    complete = null
  }
  render()
})

input.addEventListener('keydown', e => {
  if (e.isComposing) return
  const n = items.length
  if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
    e.preventDefault()
    const down = e.key === 'ArrowDown'
    if (n) glide(results, (sel = sel < 0 ? (down ? 0 : n - 1) : (sel + (down ? 1 : n - 1)) % n))
  } else if (e.key === 'Enter') {
    e.preventDefault()
    if (sel < 0) send({ t: 'close' })
    else run(sel, e.shiftKey || e.altKey || e.ctrlKey)
  } else if (e.key === 'Tab' || (e.key === 'ArrowRight' && completion())) {
    e.preventDefault()
    const c = completion()
    if (c) {
      input.value = c
      input.dispatchEvent(new InputEvent('input'))
    }
  } else if (e.key === 'Escape') {
    e.preventDefault()
    if (home) {
      input.value = ''
      render()
    } else send({ t: 'close' })
  }
})

results.addEventListener('mousemove', e => {
  const row = e.target.closest('.row')
  if (row && +row.dataset.i !== sel) glide(results, (sel = +row.dataset.i))
})
results.addEventListener('mousedown', e => e.preventDefault())
results.addEventListener('click', e => {
  const row = e.target.closest('.row')
  if (row) run(+row.dataset.i, e.shiftKey || e.ctrlKey)
})
results.addEventListener('auxclick', e => {
  const row = e.target.closest('.row')
  if (row && e.button === 1) run(+row.dataset.i, true)
})

on.results = m => {
  if (m.q !== input.value.trim()) return
  found = m.items
  complete = m.complete
  render()
}

on.suggest = m => {
  if (m.q !== input.value.trim()) return
  hints = m.items
  render()
}

const cards = $('#cards')
let picks = []
let pick = 0

function openSwitch(m, again) {
  picks = m.list
  pick = m.pick
  $('.rows', cards).innerHTML = picks
    .map(t => {
      const badges = [t.audible && icon(t.muted ? 'muted' : 'sound'), t.asleep && icon('moon')].filter(Boolean)
      return `<div class="row${t.id === m.active ? ' current' : ''}${t.asleep ? ' asleep' : ''}" data-id="${t.id}"><span class="icon">${fav(t.icon)}</span><div class="text"><div class="title">${esc(t.title || 'New tab')}</div><div class="sub">${esc(host(t.url))}</div></div>${badges.map(b => `<span class="badge">${b}</span>`).join('')}<button class="x" data-kill="${t.id}">${icon('x')}</button></div>`
    })
    .join('')
  $$('.row', cards).forEach((r, i) => r.style.setProperty('--i', i))
  if (!again) replay(cards)
  const next = keysOf('switch')[0]
  const close = keysOf('close')[0]
  $('#hold').innerHTML = [next && `<span>${kbd(next)} next</span>`, close && `<span>${kbd(close)} close</span>`, '<span>release to switch</span>']
    .filter(Boolean)
    .join('')
  glide(cards, pick)
}

on.pick = m => glide(cards, (pick = m.i))

cards.addEventListener('mousemove', e => {
  const row = e.target.closest('.row')
  const i = $$('.row', cards).indexOf(row)
  if (row && i !== pick) {
    glide(cards, (pick = i))
    send({ t: 'pick', i })
  }
})
cards.addEventListener('click', e => {
  const kill = e.target.closest('[data-kill]')
  const row = e.target.closest('.row')
  if (kill) send({ t: 'kill', id: +kill.dataset.kill })
  else if (row) send({ t: 'activate', id: +row.dataset.id })
})

const restartKeys = ['lite', 'gpu', 'processes', 'scrollbars']

const sections = [
  {
    id: 'general',
    name: 'General',
    icon: 'sliders',
    lead: 'How Xeno starts and fills your screen.',
    items: [
      { key: 'restore', type: 'toggle', label: 'Restore tabs on launch', note: 'Tabs come back asleep and load when you open them.' },
      { key: 'start', type: 'text', label: 'Start page', note: 'Opens when there is nothing to restore.', placeholder: 'Action bar' },
      { key: 'fill', type: 'choice', label: 'Window', options: [['screen', 'Full screen'], ['work', 'Keep taskbar']] },
      { key: 'zoom', type: 'range', label: 'Page zoom', min: 0.5, max: 2, step: 0.05, unit: '%' },
    ],
  },
  {
    id: 'search',
    name: 'Search',
    icon: 'search',
    lead: 'Where your searches go.',
    items: [
      { key: 'engine', type: 'choice', label: 'Search engine', options: () => [...meta.engines.map(e => [e.id, e.name]), ['custom', 'Custom']], wrap: true },
      { key: 'custom', type: 'text', label: 'Custom search address', note: 'Put %s where the search terms go.', placeholder: 'https://example.com/?q=%s', when: () => config.engine === 'custom' },
      { key: 'suggest', type: 'toggle', label: 'Search suggestions', note: 'Asks your search engine for ideas while you type.' },
    ],
  },
  {
    id: 'look',
    name: 'Appearance',
    icon: 'palette',
    lead: 'Make it yours.',
    items: [
      { key: 'theme', type: 'choice', label: 'Theme', options: [['dark', 'Dark'], ['light', 'Light'], ['system', 'System']] },
      { key: 'accent', type: 'color', label: 'Accent' },
      { key: 'motion', type: 'choice', label: 'Motion', options: [[0, 'Off'], [0.65, 'Snappy'], [1, 'Smooth'], [1.4, 'Relaxed']] },
      { key: 'depth', type: 'toggle', label: 'Depth', note: 'Pages ease back while the action bar, tabs or settings are open.' },
      { key: 'corners', type: 'range', label: 'Corner radius', min: 0, max: 28, step: 1, unit: 'px' },
      { key: 'scale', type: 'range', label: 'Interface size', min: 0.8, max: 1.3, step: 0.05, unit: '%' },
      { key: 'font', type: 'text', label: 'Interface font', placeholder: 'Segoe UI Variable' },
      { key: 'line', type: 'toggle', label: 'Loading line', note: 'A thin line along the top while pages load.' },
      { key: 'sites', type: 'choice', label: 'Websites prefer', options: [['system', 'System'], ['light', 'Light'], ['dark', 'Dark']] },
    ],
  },
  {
    id: 'tuning',
    name: 'Performance',
    icon: 'gauge',
    lead: 'Xeno only spends memory on the tabs you are actually using.',
    meter: true,
    items: [
      { key: 'sleep', type: 'choice', label: 'Sleep inactive tabs', note: 'Sleeping tabs keep their place but stop using the CPU.', options: [[0, 'Never'], [5, '5m'], [15, '15m'], [30, '30m'], [60, '1h']] },
      { key: 'discard', type: 'choice', label: 'Unload inactive tabs', note: 'Unloaded tabs free all their memory and reload when opened.', options: [[0, 'Never'], [30, '30m'], [60, '1h'], [120, '2h'], [240, '4h']] },
      { key: 'trim', type: 'toggle', label: 'Trim background tabs', note: 'Background tabs hand memory back as soon as you leave them.' },
      { key: 'lite', type: 'toggle', label: 'Lite JavaScript', note: 'Uses less memory, but scripts run slower.' },
      { key: 'gpu', type: 'toggle', label: 'Hardware acceleration', note: 'Turning this off saves memory but costs CPU.' },
      { key: 'processes', type: 'choice', label: 'Process limit', note: 'Fewer processes save memory; tabs share them.', options: [[0, 'Auto'], [2, '2'], [4, '4'], [8, '8']] },
      { key: 'scrollbars', type: 'toggle', label: 'Overlay scrollbars' },
      { type: 'buttons', items: [['Sleep background tabs now', 'sleep']] },
    ],
  },
  {
    id: 'privacy',
    name: 'Privacy',
    icon: 'shield',
    lead: 'What Xeno blocks, keeps and forgets.',
    items: [
      { key: 'tracking', type: 'choice', label: 'Tracking prevention', options: [['off', 'Off'], ['basic', 'Basic'], ['balanced', 'Balanced'], ['strict', 'Strict']] },
      { key: 'popups', type: 'toggle', label: 'Block pop-ups', note: 'Only windows you open with a click get through.' },
      { key: 'history', type: 'toggle', label: 'Keep history' },
      { key: 'autofill', type: 'toggle', label: 'Autofill and passwords' },
      { type: 'buttons', items: [['Clear history', 'clear-history'], ['Clear all browsing data', 'clear-data']] },
    ],
  },
  { id: 'extensions', name: 'Extensions', icon: 'puzzle', lead: 'Chrome extensions work here too. Their popups open as tabs.', page: extensionsPage },
  { id: 'keys', name: 'Keys', icon: 'keyboard', lead: 'Click a shortcut, then press the keys you want.', page: keysPage },
  { id: 'about', name: 'About', icon: 'info', lead: 'A minimal browser built on the web engine inside Windows.', page: aboutPage },
]

const actions = [
  ['bar', 'Action bar'],
  ['switch', 'Switch tabs'],
  ['switch_back', 'Switch tabs backwards'],
  ['new', 'New tab'],
  ['close', 'Close tab'],
  ['reopen', 'Reopen closed tab'],
  ['settings', 'Settings'],
  ['next', 'Next tab'],
  ['prev', 'Previous tab'],
  ['copy', 'Copy link'],
]

const swatches = ['#7c9cff', '#5eead4', '#a3e635', '#fbbf24', '#f87171', '#f472b6', '#c084fc', '#e4e4e7']

let section = 'general'
let shown = ''
let recording = null
let polling

const visible = s => (s.items || []).filter(i => !i.when || i.when())
const signature = () => section + visible(sections.find(s => s.id === section)).map(i => i.key).join()

function openSettings() {
  $('#nav').innerHTML =
    `<div class="brand">xeno</div><div class="hl"></div>` +
    sections.map(s => `<button data-s="${s.id}">${icon(s.icon)}<span>${s.name}</span></button>`).join('')
  place()
  renderPane()
}

function place() {
  $$('#nav button').forEach(b => b.classList.toggle('on', b.dataset.s === section))
  const b = $(`#nav button[data-s="${section}"]`)
  $('#nav .hl').style.transform = `translateY(${b.offsetTop}px)`
}

$('#nav').addEventListener('click', e => {
  const b = e.target.closest('button')
  if (!b || b.dataset.s === section) return
  section = b.dataset.s
  place()
  renderPane()
})

function label(it) {
  const tag = restartKeys.includes(it.key) ? '<span class="tag">Restart</span>' : ''
  return `<div class="label"><div>${it.label}${tag}</div>${it.note ? `<div class="note">${it.note}</div>` : ''}</div>`
}

function control(it) {
  const key = `data-key="${it.key}" data-type="${it.type}"`
  switch (it.type) {
    case 'toggle':
      return `<div class="item">${label(it)}<button class="toggle" ${key}></button></div>`
    case 'choice': {
      const options = typeof it.options === 'function' ? it.options() : it.options
      return `<div class="item${it.wrap ? ' stack' : ''}">${label(it)}<div class="choice${it.wrap ? ' wrap' : ''}" ${key}><span class="pill"></span>${options
        .map(([v, name]) => `<button data-v='${JSON.stringify(v)}'>${esc(name)}</button>`)
        .join('')}</div></div>`
    }
    case 'range':
      return `<div class="item">${label(it)}<div class="range"><input type="range" ${key} min="${it.min}" max="${it.max}" step="${it.step}" data-unit="${it.unit}"><output></output></div></div>`
    case 'text':
      return `<div class="item">${label(it)}<div class="text"><input ${key} spellcheck="false" placeholder="${esc(it.placeholder)}"></div></div>`
    case 'color':
      return `<div class="item">${label(it)}<div class="swatches" ${key}>${swatches
        .map(c => `<button data-v="${c}" style="--c:${c}"></button>`)
        .join('')}<label title="Custom"><input type="color"></label></div></div>`
    case 'buttons':
      return `<div class="buttons">${it.items.map(([name, cmd]) => `<button class="button" data-cmd="${cmd}">${name}</button>`).join('')}</div>`
  }
}

function meterHtml() {
  return `<div class="meter"><div class="big" id="mb">${usage ? usage.mb : '–'}<small>MB</small></div><div class="facts" id="facts">${usage ? facts(usage) : 'Measuring'}</div></div>`
}

const facts = u => `${u.procs} processes · ${u.live} of ${u.tabs} ${u.tabs === 1 ? 'tab' : 'tabs'} awake`

function renderPane() {
  const s = sections.find(s => s.id === section)
  const same = shown.startsWith(section)
  const pane = $('#pane')
  const top = pane.scrollTop
  const body = s.page
    ? s.page()
    : (s.meter ? meterHtml() : '') +
      visible(s).map(control).join('') +
      (s.id === 'tuning' ? '<div class="buttons" id="restart"><button class="button strong" data-cmd="restart">Restart Xeno to apply</button></div>' : '')
  pane.innerHTML = `<div class="${same ? '' : 'page'}"><h1>${s.name}</h1><p class="lead">${s.lead}</p>${body}</div>`
  if (same) pane.scrollTop = top
  else pane.scrollTop = 0
  shown = signature()
  if (s.id === 'tuning') send({ t: 'stats' })
  sync()
}

function pill(el) {
  const b = $('button.on', el)
  const p = $('.pill', el)
  if (!b) return (p.style.opacity = 0)
  p.style.opacity = 1
  p.style.width = `${b.offsetWidth}px`
  p.style.transform = `translate(${b.offsetLeft}px, ${b.offsetTop - 3}px)`
}

function fill(el) {
  const pct = ((el.value - el.min) / (el.max - el.min)) * 100
  el.style.setProperty('--fill', `${pct}%`)
  const v = +el.value
  el.nextElementSibling.textContent = el.dataset.unit === '%' ? `${Math.round(v * 100)}%` : `${v}${el.dataset.unit}`
}

function sync() {
  $$('#pane [data-key]').forEach(el => {
    const v = config[el.dataset.key]
    switch (el.dataset.type) {
      case 'toggle':
        el.classList.toggle('on', !!v)
        break
      case 'choice':
        $$('button', el).forEach(b => b.classList.toggle('on', JSON.parse(b.dataset.v) === v))
        pill(el)
        break
      case 'range':
        if (document.activeElement !== el) el.value = v
        fill(el)
        break
      case 'text':
        if (document.activeElement !== el) el.value = v ?? ''
        break
      case 'color':
        $$('button', el).forEach(b => b.classList.toggle('on', b.dataset.v.toLowerCase() === String(v).toLowerCase()))
        $('input', el).value = v
        break
    }
  })
  $('#restart')?.classList.toggle('hidden', !restartKeys.some(k => boot && config[k] !== boot[k]))
}

function set(key, value) {
  config[key] = value
  send({ t: 'set', key, value })
  apply()
  sync()
}

$('#pane').addEventListener('click', e => {
  const b = e.target.closest('button')
  if (!b) return
  const holder = b.closest('[data-key]')
  if (b.dataset.ext) return extensionAction(b)
  if (b.dataset.bind) return listen(b)
  if (b.dataset.cmd) return send({ t: 'cmd', name: b.dataset.cmd })
  if (b.matches('.toggle')) return set(b.dataset.key, !config[b.dataset.key])
  if (holder?.dataset.type === 'choice') return set(holder.dataset.key, JSON.parse(b.dataset.v))
  if (holder?.dataset.type === 'color') return set('accent', b.dataset.v)
})

$('#pane').addEventListener('input', e => {
  const el = e.target
  if (el.dataset.type === 'range') fill(el)
  if (el.type === 'color') document.documentElement.style.setProperty('--accent', el.value)
})

$('#pane').addEventListener('change', e => {
  const el = e.target
  if (el.dataset.type === 'range') set(el.dataset.key, +el.value)
  if (el.dataset.type === 'text') set(el.dataset.key, el.value.trim())
  if (el.type === 'color') set('accent', el.value)
})

$('#pane').addEventListener('keydown', e => {
  if (e.key !== 'Enter') return
  if (e.target.id === 'crx') extensionAction($('[data-ext="install"]'))
  else if (e.target.dataset.type === 'text') e.target.blur()
})

function extensionsPage() {
  const list = exts.length
    ? exts
        .map(x => {
          const open = [x.popup && ['Open', x.popup], x.options && ['Options', x.options]].filter(Boolean)
          return `<div class="ext"><div class="seal">${esc(x.name.trim()[0] || '?')}</div><div class="label"><div>${esc(x.name)}</div><div class="note">${esc(x.id)}</div></div><div class="actions">${open
            .map(([name, page]) => `<button class="button" data-ext="open" data-id="${x.id}" data-page="${esc(page)}">${name}</button>`)
            .join('')}<button class="toggle${x.enabled ? ' on' : ''}" data-ext="toggle" data-id="${x.id}"></button><button class="x" style="opacity:1" data-ext="remove" data-id="${x.id}">${icon('x')}</button></div></div>`
        })
        .join('')
    : '<div class="empty">No extensions yet.</div>'
  return `<div class="wide"><input id="crx" spellcheck="false" placeholder="Paste a Chrome Web Store link"><button class="button strong" data-ext="install">Install</button></div><div class="buttons" style="padding:0 0 14px"><button class="button" data-ext="folder">Load unpacked folder</button></div>${list}`
}

function extensionAction(b) {
  const op = b.dataset.ext
  const id = b.dataset.id
  if (op === 'install') {
    const value = $('#crx').value.trim()
    if (value) send({ t: 'ext', op, value })
    $('#crx').value = ''
  } else if (op === 'toggle') {
    b.classList.toggle('on')
    send({ t: 'ext', op, id, on: b.classList.contains('on') })
  } else send({ t: 'ext', op, id, page: b.dataset.page })
}

function keysPage() {
  const rows = actions
    .map(([id, name]) => {
      const combos = keysOf(id)
      const chips = combos.length ? combos.map(c => `<button data-bind="${id}">${esc(c)}</button>`).join('') : `<button data-bind="${id}">Set</button>`
      return `<div class="item"><div class="label">${name}</div><div class="combo">${chips}</div></div>`
    })
    .join('')
  return `${rows}<div class="item"><div class="label"><div>Jump to a tab</div><div class="note">Alt+9 always goes to the last one.</div></div><div class="combo"><button disabled>Alt+1 to Alt+9</button></div></div><div class="buttons"><button class="button" data-cmd="reset-keys">Reset to defaults</button></div>`
}

function listen(b) {
  stopListening()
  recording = b.dataset.bind
  b.classList.add('listening')
  b.textContent = 'Press keys'
  send({ t: 'record', on: true })
}

function stopListening() {
  if (!recording) return
  recording = null
  send({ t: 'record', on: false })
  if (mode === 'settings' && section === 'keys') renderPane()
}

const named = {
  ArrowLeft: 'Left', ArrowRight: 'Right', ArrowUp: 'Up', ArrowDown: 'Down', Escape: 'Esc', Comma: ',', Period: '.', Slash: '/',
  Minus: '-', Equal: '=', Semicolon: ';', BracketLeft: '[', BracketRight: ']', Quote: "'", Backquote: '`', Backslash: '\\',
  Space: 'Space', Enter: 'Enter', Tab: 'Tab', Backspace: 'Backspace', Delete: 'Delete', Home: 'Home', End: 'End', PageUp: 'PageUp', PageDown: 'PageDown',
}

function comboOf(e) {
  const c = e.code
  const key = c.startsWith('Key') ? c.slice(3) : c.startsWith('Digit') ? c.slice(5) : /^F\d+$/.test(c) ? c : named[c]
  if (!key || (!e.ctrlKey && !e.altKey && !/^F\d+$/.test(key))) return null
  return `${e.ctrlKey ? 'Ctrl+' : ''}${e.altKey ? 'Alt+' : ''}${e.shiftKey ? 'Shift+' : ''}${key}`
}

document.addEventListener(
  'keydown',
  e => {
    if (recording) {
      e.preventDefault()
      e.stopPropagation()
      if (e.key === 'Escape') return stopListening()
      const combo = comboOf(e)
      if (!combo) return
      const id = recording
      recording = null
      send({ t: 'record', on: false })
      set('keys', { ...config.keys, [id]: combo })
      return renderPane()
    }
    if (mode === 'switch') {
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        e.preventDefault()
        const n = picks.length
        pick = (pick + (e.key === 'ArrowDown' ? 1 : n - 1)) % n
        glide(cards, pick)
        send({ t: 'pick', i: pick })
      } else if (e.key === 'Enter' && picks[pick]) send({ t: 'activate', id: picks[pick].id })
      else if (e.key === 'Escape') send({ t: 'close' })
    } else if (mode === 'settings' && e.key === 'Escape' && !e.target.matches('input')) send({ t: 'close' })
  },
  true
)

function aboutPage() {
  const rows = [
    ['Xeno', meta.version],
    ['Engine', `Microsoft Edge WebView2 ${meta.runtime}`],
    ['Profile', meta.data],
  ]
  return `<div class="facts-list">${rows.map(([a, b]) => `<div><span>${a}</span><span>${esc(b)}</span></div>`).join('')}</div><div class="buttons"><button class="button" data-cmd="data">Open profile folder</button><button class="button" data-cmd="restart">Restart Xeno</button></div>`
}

$('#veil').addEventListener('mousedown', () => {
  if (!document.body.classList.contains('home')) send({ t: 'close' })
})

function apply() {
  const root = document.documentElement
  const dark = config.theme === 'dark' || (config.theme === 'system' && meta.dark)
  root.className = dark ? 'dark' : 'light'
  root.style.setProperty('--accent', config.accent)
  root.style.setProperty('--speed', config.motion)
  root.style.setProperty('--font', config.font ? `"${config.font}", var(--system)` : 'var(--system)')
  document.body.style.zoom = config.scale
  $('#primer').innerHTML = [
    ['bar', 'actions'],
    ['switch', 'tabs'],
    ['settings', 'settings'],
  ]
    .map(([id, name]) => keysOf(id)[0] && `<span>${kbd(keysOf(id)[0])} ${name}</span>`)
    .filter(Boolean)
    .join('')
  $('#tips').innerHTML = '<span><kbd>Enter</kbd> open</span><span><kbd>Shift</kbd><kbd>Enter</kbd> new tab</span><span><kbd>Tab</kbd> complete</span><span><kbd>Esc</kbd> close</span>'
}

on.config = m => {
  config = m.config
  meta = m
  boot ??= { ...config }
  apply()
  if (mode !== 'settings') return
  if (signature() !== shown) renderPane()
  else sync()
}

on.tabs = m => {
  tabs = m.tabs
  active = m.active
  mru = m.mru
  if (mode === 'bar') render()
}

on.mode = m => {
  const was = mode
  mode = m.mode
  document.body.dataset.mode = mode
  document.body.classList.toggle('home', mode === 'bar' && m.home)
  bar.classList.toggle('on', mode === 'bar')
  $('#switch').classList.toggle('on', mode === 'switch')
  $('#settings').classList.toggle('on', mode === 'settings')
  if (mode !== 'none') $('#peek').classList.remove('on')
  if (mode === 'bar') openBar(m)
  else input.blur()
  if (mode === 'switch') openSwitch(m, was === 'switch')
  if (mode === 'settings' && was !== 'settings') openSettings()
  if (mode !== 'settings') stopListening()
  clearInterval(polling)
  if (mode === 'settings') polling = setInterval(() => section === 'tuning' && send({ t: 'stats' }), 2000)
}

on.section = m => {
  section = m.id
  if (mode === 'settings') {
    place()
    renderPane()
  }
}

on.exts = m => {
  exts = m.items
  if (mode === 'settings' && section === 'extensions') renderPane()
}

on.stats = m => {
  usage = m
  $('#usage').textContent = `${m.mb} MB`
  if ($('#mb')) {
    $('#mb').innerHTML = `${m.mb}<small>MB</small>`
    $('#facts').textContent = facts(m)
  }
}

on.status = m => {
  const peek = $('#peek')
  if (m.text) peek.textContent = m.text
  peek.classList.toggle('on', !!m.text && mode === 'none')
}

let toasting
on.toast = m => {
  const t = $('#toast')
  t.textContent = m.text
  t.classList.remove('on')
  void t.offsetWidth
  t.classList.add('on')
  clearTimeout(toasting)
  toasting = setTimeout(() => t.classList.remove('on'), 2600)
}

send({ t: 'ready' })
