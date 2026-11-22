/* Pulse — interface (JS puro, sem build). Todo dado dinâmico passa por esc(). */
(() => {
  'use strict';

  // ------------------------------------------------------------ utilidades
  const $ = (s, r = document) => r.querySelector(s);
  const $$ = (s, r = document) => Array.from(r.querySelectorAll(s));
  const ESC = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' };
  const esc = (s) => String(s ?? '').replace(/[&<>"']/g, (c) => ESC[c]);
  const store = {
    get(k) { try { return localStorage.getItem(k); } catch { return null; } },
    set(k, v) { try { localStorage.setItem(k, v); } catch { /* sem armazenamento */ } },
  };

  // ------------------------------------------------------------ idioma (pt-BR / en)
  // A frase em pt-BR é a chave; EN (no fim do arquivo) traz a tradução. Placeholders: {nome}.
  const detectLang = () => { try { return /^pt/i.test(navigator.language || '') ? 'pt' : 'en'; } catch { return 'pt'; } };
  let LANG = store.get('pulse.lang') === 'en' ? 'en' : store.get('pulse.lang') === 'pt' ? 'pt' : detectLang();
  const LOC = () => (LANG === 'en' ? 'en-US' : 'pt-BR');
  function t(s, vars) {
    let r = LANG === 'en' ? (EN[s] ?? s) : s;
    if (vars) r = r.replace(/\{(\w+)\}/g, (m, k) => (k in vars ? String(vars[k]) : m));
    return r;
  }
  // Mensagens do backend com partes variáveis (as fixas passam direto por t()).
  const EN_RX = [
    [/^prioridade ajustada para (-?\d+)$/, 'priority set to $1'],
    [/^muitas tentativas com token inválido; tente de novo em (\d+) s$/, 'too many attempts with an invalid token; try again in $1 s'],
    [/^JSON inválido: (.*)$/, 'invalid JSON: $1'],
    [/^(.+) precisa ser número$/, '$1 must be a number'],
    [/^(.+) precisa ser texto$/, '$1 must be text'],
    [/^(.+) precisa ser true ou false$/, '$1 must be true or false'],
    [/^valor inválido em (.+)$/, 'invalid value in $1'],
    [/^nome de modelo inválido: (.*)$/, 'invalid model name: $1'],
    [/^preço de (.+) precisa ter 4 números ≥ 0$/, 'price of $1 needs 4 numbers ≥ 0'],
    [/^não foi possível gravar a configuração: (.*)$/, 'could not write the configuration: $1'],
    [/^chave desconhecida: (.*)$/, 'unknown key: $1'],
    [/^(.+) deve estar entre (.+) e (.+)$/, '$1 must be between $2 and $3'],
    [/^(.+) é somente leitura aqui: abra o Pulse dessa máquina para executar ações$/, '$1 is read-only here: open that machine\'s Pulse to run actions'],
    [/^RAPL indisponível: (.*)$/, 'RAPL unavailable: $1'],
    [/^já existe (?:outro|um) .*notify.*Codex.*$/i, 'another notify is already set in the Codex config; nothing was changed'],
    [/^(.+) indisponível$/, '$1 unavailable'],
    [/^não foi possível ler (.+?): (.*)$/, 'could not read $1: $2'],
    [/^o início do config\.toml tem strings multilinha.*$/, 'the top of config.toml has multi-line strings; add the notify line by hand'],
    // regras e alertas (pulse-core/src/alerts.rs)
    [/^CPU > (\S+)% por (\d+) amostras seguidas$/, 'CPU > $1% for $2 consecutive samples'],
    [/^memória > (\S+)%$/, 'memory > $1%'],
    [/^uso > (\S+)%$/, 'usage > $1%'],
    [/^processo > (\S+)% CPU por (\d+) amostras$/, 'process > $1% CPU for $2 samples'],
    [/^(\d+)% sustentado$/, '$1% sustained'],
    [/^(\d+)% em uso$/, '$1% in use'],
    [/^(.+) \(PID (\d+)\) em (\d+)% de CPU$/, '$1 (PID $2) at $3% CPU'],
  ];
  function tErr(s) {
    s = String(s ?? '');
    if (LANG !== 'en') return s;
    if (EN[s] != null) return EN[s];
    for (const [rx, rep] of EN_RX) if (rx.test(s)) return s.replace(rx, rep);
    return s;
  }
  const fmtCache = {};
  const nf = (d) => (fmtCache[LOC() + d] ||= new Intl.NumberFormat(LOC(), { minimumFractionDigits: d, maximumFractionDigits: d }));
  const num = (v, d = 0) => (v == null || Number.isNaN(v) ? '—' : nf(d).format(v));
  const pct = (v, d = 1) => (v == null || Number.isNaN(v) ? '—' : nf(d).format(v) + '%');
  function bytes(b, d) {
    if (b == null || Number.isNaN(b)) return '—';
    const u = ['B', 'KB', 'MB', 'GB', 'TB', 'PB'];
    let i = 0;
    let v = Math.abs(b);
    while (v >= 1024 && i < u.length - 1) { v /= 1024; i++; }
    const dd = d ?? (i === 0 ? 0 : v < 10 ? 1 : 0);
    return nf(dd).format(Math.sign(b) * v) + ' ' + u[i];
  }
  const rate = (b) => bytes(b) + '/s';
  const kilo = (n) => (n == null ? '—' : n >= 1e6 ? num(n / 1e6, 1) + 'M' : n >= 1e3 ? num(n / 1e3, 1) + 'k' : num(n));
  function dur(s) {
    s = Math.max(0, Math.floor(s || 0));
    const d = Math.floor(s / 86400), h = Math.floor((s % 86400) / 3600), m = Math.floor((s % 3600) / 60), x = s % 60;
    if (d) return `${d}d ${h}h`;
    if (h) return `${h}h ${String(m).padStart(2, '0')}m`;
    if (m) return `${m}m ${String(x).padStart(2, '0')}s`;
    return `${x}s`;
  }
  function ago(ms) {
    if (!ms) return '—';
    const s = (Date.now() - ms) / 1000;
    if (s < 5) return t('agora');
    const v = s < 60 ? Math.floor(s) + 's' : s < 3600 ? Math.floor(s / 60) + 'm' : s < 86400 ? Math.floor(s / 3600) + 'h' : Math.floor(s / 86400) + 'd';
    return t('há {v}', { v });
  }
  const clock = (ms) => new Date(ms).toLocaleTimeString(LOC());
  const dateTime = (ms) => new Date(ms).toLocaleString(LOC(), { dateStyle: 'short', timeStyle: 'medium' });
  const lvl = (v, w, c) => (v > c ? 'crit' : v > w ? 'warn' : '');
  const session = {
    get(k) { try { return sessionStorage.getItem(k); } catch { return null; } },
    set(k, v) { try { sessionStorage.setItem(k, v); } catch { /* idem */ } },
    del(k) { try { sessionStorage.removeItem(k); } catch { /* idem */ } },
  };

  // ------------------------------------------------------------ ícones
  const I = (d) => `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${d}</svg>`;
  const ICONS = {
    grid: I('<rect x="3.5" y="3.5" width="7" height="7" rx="2"/><rect x="13.5" y="3.5" width="7" height="7" rx="2"/><rect x="3.5" y="13.5" width="7" height="7" rx="2"/><rect x="13.5" y="13.5" width="7" height="7" rx="2"/>'),
    cpu: I('<rect x="6" y="6" width="12" height="12" rx="2"/><path d="M9 2v4M15 2v4M9 18v4M15 18v4M2 9h4M2 15h4M18 9h4M18 15h4"/>'),
    mem: I('<rect x="2.5" y="7" width="19" height="10" rx="2"/><path d="M6.5 10v4M10 10v4M14 10v4M17.5 10v4"/>'),
    disk: I('<ellipse cx="12" cy="6" rx="8" ry="3"/><path d="M4 6v12c0 1.7 3.6 3 8 3s8-1.3 8-3V6M4 12c0 1.7 3.6 3 8 3s8-1.3 8-3"/>'),
    list: I('<path d="M8 6h13M8 12h13M8 18h13M3.5 6h.01M3.5 12h.01M3.5 18h.01"/>'),
    gear: I('<circle cx="12" cy="12" r="3"/><path d="M12 2v3M12 19v3M4.9 4.9l2.1 2.1M17 17l2.1 2.1M2 12h3M19 12h3M4.9 19.1L7 17M17 7l2.1-2.1"/>'),
    globe: I('<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3c2.5 2.7 3.8 5.7 3.8 9s-1.3 6.3-3.8 9c-2.5-2.7-3.8-5.7-3.8-9S9.5 5.7 12 3z"/>'),
    doc: I('<path d="M6 3h9l4 4v14H6z"/><path d="M9 9h6M9 13h6M9 17h4"/>'),
    bot: I('<rect x="4" y="8" width="16" height="11" rx="3"/><path d="M12 4v4M9 13h.01M15 13h.01M9.5 16h5"/>'),
    bell: I('<path d="M6 16V11a6 6 0 1 1 12 0v5l2 2H4z"/><path d="M10 20a2 2 0 0 0 4 0"/>'),
  };

  // ------------------------------------------------------------ estado
  const S = {
    snap: null,
    hist: { ts: [], cpu: [], mem: [], swap: [], rx: [], tx: [], rd: [], wr: [], temp: [], gpu: [] },
    lastSeq: 0,
    page: null,
    agents: null,
    es: null,
    remote: session.get('pulse.remote') || '',
    remotes: [],
  };
  const HIST_MAX = 900;

  function pushPoint(s) {
    if (s.seq && s.seq === S.lastSeq) return;
    S.lastSeq = s.seq;
    const h = S.hist;
    const p = {
      ts: s.ts, cpu: s.cpu.total, mem: s.mem.pct, swap: s.mem.swap_pct,
      rx: s.net.rx_bps, tx: s.net.tx_bps, rd: s.disks.read_bps, wr: s.disks.write_bps,
      temp: maxTemp(s),
      gpu: s.gpus && s.gpus.length ? s.gpus[0].busy_pct : null,
    };
    for (const k in h) { h[k].push(p[k]); if (h[k].length > HIST_MAX) h[k].shift(); }
  }
  const maxTemp = (s) => (s.temps && s.temps.length ? Math.max(...s.temps.map((t) => t.c)) : null);

  // ------------------------------------------------------------ API
  const token = () => session.get('pulse.token');
  // Máquina remota selecionada: GETs de /api/ passam pelo repasse somente leitura.
  const remotePath = (path) => (S.remote && path.startsWith('/api/') && path !== '/api/remotes' ? `/api/remote/${encodeURIComponent(S.remote)}${path}` : path);
  async function api(path, opts = {}) {
    const headers = {};
    const method = opts.method || 'GET';
    if (S.remote && method !== 'GET' && !opts.local) {
      throw new Error(t('{m} é somente leitura aqui: abra o Pulse dessa máquina para executar ações', { m: S.remote }));
    }
    if (!opts.local) path = remotePath(path);
    if (method !== 'GET') {
      if (!token()) await askToken(t('Para executar ações, informe o token de acesso.'));
      headers['X-Pulse-Token'] = token() || '';
      headers['Content-Type'] = 'application/json';
    }
    const r = await fetch(path, {
      method, headers, credentials: 'same-origin',
      body: opts.body !== undefined ? JSON.stringify(opts.body) : undefined,
    });
    let j = null;
    try { j = await r.json(); } catch { /* corpo vazio */ }
    if (!r.ok) {
      if (r.status === 401 && method === 'GET') { showLogin(); }
      if (r.status === 401 && method !== 'GET') { session.del('pulse.token'); }
      throw new Error(tErr((j && j.error) || `HTTP ${r.status}`));
    }
    return j;
  }

  async function login(tok) {
    const r = await fetch('/api/login', { method: 'POST', headers: { 'X-Pulse-Token': tok }, credentials: 'same-origin' });
    if (!r.ok) throw new Error(t('token inválido'));
    session.set('pulse.token', tok);
  }

  // ------------------------------------------------------------ diálogos
  let dialogResolve = null;
  function dialog({ title, body, actions, wide }) {
    const ov = $('#overlay');
    $('.dialog', ov).classList.toggle('wide', !!wide);
    $('#dlg-title').textContent = title;
    $('#dlg-body').innerHTML = body;
    const box = $('#dlg-actions');
    box.innerHTML = '';
    return new Promise((resolve) => {
      dialogResolve = resolve;
      for (const a of actions) {
        const b = document.createElement('button');
        b.className = 'btn ' + (a.cls || '');
        b.textContent = a.label;
        b.addEventListener('click', () => { if (a.value !== undefined) closeDialog(a.value); });
        if (a.onclick) b.addEventListener('click', a.onclick);
        box.appendChild(b);
      }
      ov.hidden = false;
      const f = $('#dlg-body input, #dlg-actions .primary, #dlg-actions .danger', ov) || box.lastChild;
      f && f.focus();
    });
  }
  function closeDialog(v) {
    $('#overlay').hidden = true;
    const r = dialogResolve;
    dialogResolve = null;
    r && r(v);
  }
  document.addEventListener('keydown', (e) => { if (e.key === 'Escape' && !$('#overlay').hidden) closeDialog(null); });
  $('#overlay').addEventListener('click', (e) => { if (e.target.id === 'overlay') closeDialog(null); });

  const confirmAction = (title, html, label, cls = 'danger') =>
    dialog({ title, body: html, actions: [{ label: t('Cancelar'), value: false }, { label, cls, value: true }] });

  let toastTimer = 0;
  function toast(msg, err = false) {
    const t = $('#toast');
    t.textContent = msg;
    t.className = 'toast' + (err ? ' err' : '');
    t.hidden = false;
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => { t.hidden = true; }, 4200);
  }

  async function askToken(msg) {
    while (true) {
      const ok = await dialog({
        title: t('Acesso ao Pulse'),
        body: `<p class="sub">${esc(msg)}</p>
          <label class="field">Token<input id="tok" class="input mono" type="password" autocomplete="off" spellcheck="false" placeholder="${esc(t('cole o token aqui'))}"></label>
          <p class="muted">${t('Obtenha a URL de acesso com {a} ou leia {b}.', { a: '<code class="cmd">pulse --print-token</code>', b: '<code class="cmd">~/.config/pulse/token</code>' })}</p>`,
        actions: [{ label: t('Entrar'), cls: 'primary', onclick: () => closeDialog($('#tok').value.trim()) }],
      });
      if (!ok) { if (token()) return; continue; }
      try { await login(ok); toast(t('Sessão iniciada.')); return; } catch { toast(t('Token inválido.'), true); }
    }
  }
  let loginShown = false;
  async function showLogin() {
    if (loginShown) return;
    loginShown = true;
    await askToken(t('Informe o token de acesso para ver o painel.'));
    loginShown = false;
    startStream();
    route();
  }

  // ------------------------------------------------------------ gráficos
  function ring(v, color) {
    const r = 27, c = 2 * Math.PI * r, p = Math.max(0, Math.min(100, v || 0));
    return `<svg class="ring" viewBox="0 0 64 64" role="img" aria-label="${esc(pct(v, 0))}">
      <circle cx="32" cy="32" r="${r}" fill="none" stroke="var(--trilho)" stroke-width="7"/>
      <circle cx="32" cy="32" r="${r}" fill="none" stroke="${color}" stroke-width="7" stroke-linecap="round"
        stroke-dasharray="${(c * p) / 100} ${c}" transform="rotate(-90 32 32)"/>
      <text x="32" y="37" text-anchor="middle" style="font-size:${p >= 99.5 ? 13 : 15}px">${esc(num(p, 0))}%</text></svg>`;
  }

  /** Desenha um gráfico de área/linha em SVG dentro de `el`. */
  function chart(el, { series, ts, max, fmt = (v) => pct(v, 0), height = 200, spanLabel = '−15 min' }) {
    if (!el) return;
    const W = Math.max(280, el.clientWidth || 600), H = height;
    const R = 8, T = 10, B = 22;
    const n = Math.max(...series.map((s) => s.data.length), 0);
    let top = max;
    if (top === 'auto' || top == null) {
      top = Math.max(1, ...series.flatMap((s) => s.data.filter((x) => x != null && !Number.isNaN(x))));
      const mag = Math.pow(10, Math.floor(Math.log10(top)));
      top = Math.ceil(top / mag) * mag;
    }
    // margem esquerda conforme o maior rótulo do eixo (≈6,5 px por caractere a 11 px)
    const L = Math.max(40, Math.ceil(Math.max(...[0, 0.5, 1].map((f) => fmt(top * f).length)) * 6.5) + 14);
    const x = (i) => L + (n <= 1 ? 0 : (i * (W - L - R)) / (n - 1));
    const y = (v) => T + (H - T - B) * (1 - Math.min(1, Math.max(0, v / top)));
    let g = '';
    for (const f of [0, 0.5, 1]) {
      const yy = y(top * f);
      g += `<line class="grid-line" x1="${L}" x2="${W - R}" y1="${yy}" y2="${yy}"/><text x="${L - 8}" y="${yy + 4}" text-anchor="end">${esc(fmt(top * f))}</text>`;
    }
    for (const f of [0.25, 0.75]) { const yy = y(top * f); g += `<line class="grid-line" x1="${L}" x2="${W - R}" y1="${yy}" y2="${yy}" opacity=".5"/>`; }
    let paths = '';
    for (const s of series) {
      let d = '', started = false;
      s.data.forEach((v, i) => {
        if (v == null || Number.isNaN(v)) { started = false; return; }
        d += `${started ? 'L' : 'M'}${x(i).toFixed(1)},${y(v).toFixed(1)}`;
        started = true;
      });
      if (!d) continue;
      if (s.fill !== false) {
        const firstI = s.data.findIndex((v) => v != null && !Number.isNaN(v));
        let lastI = s.data.length - 1;
        while (lastI > 0 && (s.data[lastI] == null || Number.isNaN(s.data[lastI]))) lastI--;
        paths += `<path d="${d}L${x(lastI).toFixed(1)},${y(0)}L${x(firstI).toFixed(1)},${y(0)}Z" fill="${s.color}" opacity=".16"/>`;
      }
      paths += `<path d="${d}" fill="none" stroke="${s.color}" stroke-width="2" stroke-linejoin="round" stroke-linecap="round"${s.dash ? ' stroke-dasharray="5 4" opacity=".75"' : ''}/>`;
    }
    const first = ts && ts.length ? ts[0] : null;
    const leftLbl = first && spanLabel === 'auto' ? dateTime(first) : spanLabel;
    g += `<text x="${L}" y="${H - 4}">${esc(leftLbl)}</text><text x="${W - R}" y="${H - 4}" text-anchor="end">${esc(t('agora'))}</text>`;
    el.innerHTML = `<svg class="chart" viewBox="0 0 ${W} ${H}" width="${W}" height="${H}" role="img" aria-label="${esc(t('gráfico'))}">${g}${paths}</svg>`;
  }

  // ------------------------------------------------------------ páginas
  const PAGES = [];
  const page = (id, title, icon, def) => PAGES.push({ id, title, icon, ...def });

  // títulos de card/panel, rótulos de KPI e pills passam por t() (chave pt-BR).
  const card = (title, body, extra = '') => `<section class="card glass">${title ? `<h2>${t(title)}${extra}</h2>` : ''}${body}</section>`;
  const panel = (title, body, extra = '') => `<section class="panel glass">${title ? `<h2>${t(title)}${extra}</h2>` : ''}${body}</section>`;
  const kpi = (id, ringHtml = '') => `<section class="card glass kpi" id="${id}">${ringHtml}<div><div class="lbl"></div><div class="val"></div><div class="det"></div></div></section>`;
  function setKpi(id, label, val, det, ringSvg) {
    const el = document.getElementById(id);
    if (!el) return;
    const r = el.querySelector('.ring');
    if (ringSvg !== undefined) {
      if (r) r.outerHTML = ringSvg; else el.insertAdjacentHTML('afterbegin', ringSvg);
    }
    el.querySelector('.lbl').textContent = t(label);
    el.querySelector('.val').textContent = val;
    el.querySelector('.det').textContent = det;
  }
  const html = (id, h) => { const el = document.getElementById(id); if (el) el.innerHTML = h; };
  const barHtml = (v, cls = '') => `<div class="bar"><i class="${cls}" style="width:${Math.max(0, Math.min(100, v || 0)).toFixed(1)}%"></i></div>`;
  const pill = (s, cls = '') => `<span class="pill ${cls}">${esc(t(s))}</span>`;
  const rawPill = (s, cls = '') => `<span class="pill ${cls}">${esc(s)}</span>`; // dado do sistema, sem tradução
  const sevPill = (s) => (s === 'critico' ? pill('Crítico', 'crit') : pill('Aviso', 'warn'));
  const rootFs = (s) => s.disks.fs.find((f) => f.mount === '/') || s.disks.fs[0];
  const kvList = (rows) => rows.map(([k, v]) => `<dt>${esc(t(k))}</dt><dd>${esc(v)}</dd>`).join('');
  const empty = (s, vars) => `<p class="empty">${esc(t(s, vars))}</p>`;
  const errBox = (e) => `<p class="empty">${esc(t('Erro: {m}', { m: e.message }))}</p>`;

  function consumersTable(list, memFirst) {
    if (!list || !list.length) return empty('Sem dados.');
    return `<div class="table-wrap"><table><thead><tr><th>${t('Processo')}</th><th class="num">CPU</th><th class="num">${t('Memória')}</th></tr></thead><tbody>
      ${list.map((p) => `<tr><td class="trunc-s" title="${esc(p.cmd)}">${esc(p.name)} <span class="muted">· ${esc(p.pid)}</span></td><td class="num">${pct(p.cpu)}</td><td class="num">${memFirst ? '<b>' + bytes(p.rss) + '</b>' : bytes(p.rss)}</td></tr>`).join('')}
    </tbody></table></div>`;
  }

  // ---- 1. Visão geral
  let longRange = store.get('pulse.range') || '15m';
  let longData = null;
  let longTimer = 0;
  let compare = store.get('pulse.compare') === '1';
  const SPAN = { '1h': 3600e3, '24h': 86400e3, '7d': 7 * 86400e3 };
  const PREV = { '1h': 'hora anterior', '24h': 'ontem', '7d': 'semana anterior' };
  const canCompare = () => compare && SPAN[longRange];
  async function loadLong() {
    if (longRange === '15m') { longData = null; return; }
    // comparar precisa do dobro da janela: 1 h/24 h vêm da camada de 1 min (7 d), 7 d da de 15 min (30 d)
    const range = canCompare() ? (longRange === '7d' ? '30d' : '7d') : longRange;
    try { longData = await api('/api/history/long?range=' + range); } catch (e) { toast(t('Histórico longo: {m}', { m: e.message }), true); }
  }
  /** Reamostra (ts, valores) em `n` baldes iguais entre start e end (média por balde). */
  function resample(ts, vals, start, end, n) {
    const sum = new Array(n).fill(0), cnt = new Array(n).fill(0), w = (end - start) / n;
    for (let i = 0; i < ts.length; i++) {
      const t = ts[i], v = vals[i];
      if (t < start || t >= end || v == null || Number.isNaN(v)) continue;
      const b = Math.min(n - 1, Math.floor((t - start) / w));
      sum[b] += v; cnt[b]++;
    }
    return sum.map((v, i) => (cnt[i] ? v / cnt[i] : null));
  }
  function drawMainChart() {
    const el = $('#ov-chart');
    if (!el) return;
    if (canCompare() && longData) {
      const p = longData.points, span = SPAN[longRange], end = Date.now(), n = 120;
      const now = (k) => resample(p.ts, p[k], end - span, end, n);
      const before = (k) => resample(p.ts, p[k], end - 2 * span, end - span, n);
      chart(el, {
        series: [
          { data: before('cpu'), color: 'var(--acento)', fill: false, dash: true },
          { data: before('mem'), color: 'var(--violeta)', fill: false, dash: true },
          { data: now('cpu'), color: 'var(--acento)' }, { data: now('mem'), color: 'var(--violeta)' },
        ],
        max: 100, spanLabel: '−' + longRange.replace('h', ' h').replace('d', ' d'),
      });
      const avg = (a) => { const v = a.filter((x) => x != null); return v.length ? v.reduce((x, y) => x + y, 0) / v.length : null; };
      const c1 = avg(now('cpu')), c0 = avg(before('cpu'));
      html('ov-chart-note', esc(t('tracejado = {p} · CPU média {a} agora × {b} antes', { p: t(PREV[longRange]), a: pct(c1, 0), b: pct(c0, 0) })));
      return;
    }
    if (longRange === '15m' || !longData) {
      chart(el, { series: [{ data: S.hist.cpu, color: 'var(--acento)' }, { data: S.hist.mem, color: 'var(--violeta)' }], ts: S.hist.ts, max: 100 });
    } else {
      const p = longData.points;
      chart(el, { series: [{ data: p.cpu, color: 'var(--acento)' }, { data: p.mem, color: 'var(--violeta)' }], ts: p.ts, max: 100, spanLabel: p.ts.length ? 'auto' : '—' });
    }
    const n = longRange === '15m' ? S.hist.ts.length : longData ? longData.count : 0;
    const bucket = longData && longData.bucket_ms >= 900000 ? '15 min' : '60 s';
    const cmp = $('#ov-compare-wrap');
    if (cmp) cmp.hidden = !SPAN[longRange];
    html('ov-chart-note', esc(longRange === '15m' ? t('{n} amostras de 1 s', { n: num(n) }) : t('{n} médias de {b} (histórico longo)', { n: num(n), b: bucket })));
  }

  page('visao-geral', 'Visão geral', 'grid', {
    mount(el) {
      el.innerHTML = `
        <div class="grid g3">${kpi('k-cpu', ring(0, 'var(--acento)'))}${kpi('k-mem', ring(0, 'var(--violeta)'))}${kpi('k-swap', ring(0, 'var(--violeta)'))}</div>
        <div class="grid g3">${kpi('k-disk', ring(0, 'var(--acento)'))}${kpi('k-net')}${kpi('k-temp')}</div>
        <div class="grid g-main">
          ${card('CPU e memória', `<div id="ov-chart"></div><div class="toolbar"><div class="series-legend"><span><i class="dot" style="background:var(--acento)"></i>CPU</span><span><i class="dot" style="background:var(--violeta)"></i>${t('Memória')}</span></div><label class="chk" id="ov-compare-wrap" ${SPAN[longRange] ? '' : 'hidden'}><input type="checkbox" id="ov-compare" ${compare ? 'checked' : ''}> ${t('comparar com o período anterior')}</label><span class="sp"></span><span class="muted" id="ov-chart-note"></span></div>`,
            `<span class="sp"></span><span class="seg" id="ov-range">${['15m', '1h', '24h', '7d', '30d', '90d'].map((r) => `<button data-r="${r}" aria-pressed="${r === longRange}">${r === '15m' ? '15 min' : r.replace('h', ' h').replace('d', ' d')}</button>`).join('')}</span>`)}
          ${card('Alertas', '<div id="ov-alerts"></div>')}
        </div>
        <div class="grid g2">
          ${card('Maiores consumidores', '<div id="ov-top"></div>')}
          ${card('Agentes agora', `<div id="ov-agents">${empty('Carregando…')}</div>`, `<span class="sp"></span><a class="chip" href="#/agentes">${t('ver agentes')}</a>`)}
        </div>
        ${card('Armazenamento', '<div id="ov-storage"></div>')}`;
      $('#ov-compare').addEventListener('change', async (e) => {
        compare = e.target.checked;
        store.set('pulse.compare', compare ? '1' : '0');
        await loadLong();
        drawMainChart();
      });
      $('#ov-range').addEventListener('click', async (e) => {
        const b = e.target.closest('button');
        if (!b) return;
        longRange = b.dataset.r;
        store.set('pulse.range', longRange);
        $$('#ov-range button').forEach((x) => x.setAttribute('aria-pressed', String(x === b)));
        await loadLong();
        drawMainChart();
      });
      loadLong().then(drawMainChart);
      longTimer = setInterval(() => { if (longRange !== '15m') loadLong().then(drawMainChart); }, 60000);
      pollAgents();
    },
    unmount() { clearInterval(longTimer); },
    update(s) {
      const fs = rootFs(s);
      setKpi('k-cpu', 'CPU', pct(s.cpu.total), t('carga {l} · {n} núcleos', { l: num(s.cpu.load[0], 2), n: s.host.cores }), ring(s.cpu.total, 'var(--acento)'));
      setKpi('k-mem', 'Memória', bytes(s.mem.used), t('de {t} · {p}', { t: bytes(s.mem.total), p: pct(s.mem.pct, 0) }), ring(s.mem.pct, 'var(--violeta)'));
      setKpi('k-swap', 'Swap', s.mem.swap_total ? bytes(s.mem.swap_used) : '—', s.mem.swap_total ? t('de {t}', { t: bytes(s.mem.swap_total) }) : t('swap desativada'), ring(s.mem.swap_pct, 'var(--violeta)'));
      if (fs) setKpi('k-disk', 'Disco', pct(fs.pct, 0), t('{a} livres em {m}', { a: bytes(fs.avail), m: fs.mount }), ring(fs.pct, fs.pct > 90 ? 'var(--aviso)' : 'var(--acento)'));
      setKpi('k-net', 'Rede', '↓ ' + rate(s.net.rx_bps), '↑ ' + rate(s.net.tx_bps));
      const tc = maxTemp(s);
      const hot = s.temps.slice().sort((a, b) => b.c - a.c)[0];
      setKpi('k-temp', 'Temperatura', tc == null ? '—' : num(tc, 0) + ' °C', hot ? `${hot.chip} · ${hot.label}` : t('sem sensores disponíveis'));
      if (longRange === '15m') drawMainChart();
      html('ov-alerts', s.alerts.length
        ? s.alerts.map((a) => `<div class="toolbar" style="margin-bottom:8px">${sevPill(a.severity)}<b>${esc(tErr(a.title))}</b><span class="sub">${esc(tErr(a.detail))}</span></div>`).join('')
        : empty('Nenhum alerta ativo.'));
      html('ov-top', consumersTable(s.procs.top.slice(0, 6)));
      html('ov-storage', s.disks.fs.map((f) => `
        <div class="toolbar" style="margin-bottom:12px;flex-wrap:nowrap">
          <div style="width:200px;min-width:0"><div class="trunc-s"><b>${esc(f.mount)}</b></div><div class="muted" style="font-size:12px">${esc(f.fstype)} · ${esc(f.device)}</div></div>
          <div style="flex:1">${barHtml(f.pct, lvl(f.pct, 90, 97))}</div>
          <div class="sub num" style="width:190px">${bytes(f.used)} / ${bytes(f.total)} · ${pct(f.pct, 0)}</div>
        </div>`).join('') || empty('Nenhum sistema de arquivos.'));
      renderAgentsNow();
    },
  });

  const HOOK_STATE = {
    executando: ['Executando', 'ok'],
    atencao: ['Pedindo atenção', 'warn'],
    aguardando: ['Aguardando', ''],
    concluido: ['Concluído', 'info'],
  };
  const statePill = (st) => { const [l, c] = HOOK_STATE[st] || [st, '']; return pill(l, c); };

  let agentsTimer = 0;
  async function pollAgents() {
    clearTimeout(agentsTimer);
    try { S.agents = await api('/api/agents'); } catch { /* ignora */ }
    if (S.page && S.page.id === 'agentes') renderAgentsPage();
    renderAgentsNow();
    if (S.page && (S.page.id === 'agentes' || S.page.id === 'visao-geral')) agentsTimer = setTimeout(pollAgents, 4000);
  }
  function renderAgentsNow() {
    const el = $('#ov-agents');
    if (!el || !S.agents) return;
    if (!S.agents.enabled) { el.innerHTML = empty('Detecção de agentes desativada (--no-agents).'); return; }
    const hs = S.agents.hooks.sessions.filter((h) => Date.now() - h.ts < 6 * 3600e3).slice(0, 4);
    const ps = S.agents.processes;
    const sess = S.agents.sessions.slice(0, 3);
    let out = '';
    if (hs.length) out += hs.map((h) => `<div class="toolbar" style="margin-bottom:8px"><b>${esc(agentName(h.agent))}</b>${statePill(h.state)}<span class="sub">${esc(h.tool || h.event)} · ${ago(h.ts)}</span></div>`).join('');
    if (sess.length) out += sess.map((x) => `<div class="toolbar" style="margin-bottom:8px"><b>${esc(x.agent)}</b><span class="sub trunc-s">${esc(x.project_name || x.project)}</span><span class="muted">${esc(x.model || '')} · ${ago(x.updated)}</span></div>`).join('');
    if (!out) out = empty('Nenhuma sessão recente.');
    const names = [...new Set(ps.map((p) => p.agent))].join(', ');
    out += `<p class="sub">${esc(ps.length === 0 ? t('Nenhum processo de agente em execução.') : ps.length === 1 ? t('1 processo de agente em execução: {a}.', { a: names }) : t('{n} processos de agente em execução: {a}.', { n: ps.length, a: names }))}</p>`;
    el.innerHTML = out;
  }
  const agentName = (a) => ({ claude: 'Claude Code', codex: 'Codex', openclaw: 'OpenClaw', ponto: 'Ponto' })[String(a).toLowerCase()] || a;

  // ---- 2. CPU
  page('cpu', 'CPU', 'cpu', {
    mount(el) {
      el.innerHTML = `
        <div class="grid g-main">${card('Histórico', '<div id="cpu-chart"></div>')}${card('Detalhes', '<dl class="kv" id="cpu-kv"></dl>')}</div>
        ${panel('Núcleos', '<div class="tiles" id="cpu-cores"></div>', '<span class="sp"></span><span class="muted" id="cpu-model" style="text-transform:none;letter-spacing:0;font-weight:500"></span>')}
        ${panel('Temperaturas', '<div class="tiles" id="cpu-temps"></div>')}
        <div id="cpu-gpu-wrap"></div>
        <div id="cpu-power-wrap"></div>`;
    },
    update(s) {
      const c = s.cpu;
      chart($('#cpu-chart'), { series: [{ data: S.hist.cpu, color: 'var(--acento)' }], ts: S.hist.ts, max: 100, height: 220 });
      const psi = c.psi ? `${pct(c.psi.some10)} (10 s)` : t('indisponível');
      html('cpu-kv', kvList([
        ['Uso total', pct(c.total)], ['Usuário', pct(c.user)], ['Sistema', pct(c.system)], ['I/O wait', pct(c.iowait)],
        ['IRQ / steal', `${pct(c.irq)} · ${pct(c.steal)}`],
        ['Carga 1/5/15 min', c.load.map((v) => num(v, 2)).join(' · ')], ['Executando / bloqueados', `${c.procs_running} / ${c.procs_blocked}`],
        ['Trocas de contexto', kilo(c.ctxt_ps) + '/s'], ['Pressão (PSI)', psi], ['Governor', c.governor || '—'],
      ]));
      $('#cpu-model').textContent = c.model || '';
      html('cpu-cores', c.cores.map((k) => `<div class="tile"><div class="t">${esc(t('núcleo {n}', { n: k.id }))}${k.mhz ? ' · ' + num(k.mhz, 0) + ' MHz' : ''}</div><div class="v">${pct(k.pct, 0)}</div>${barHtml(k.pct, lvl(k.pct, 70, 90))}</div>`).join(''));
      html('cpu-temps', s.temps.length
        ? s.temps.map((x) => { const ref = x.crit || 100; return `<div class="tile"><div class="t" title="${esc(x.chip)}">${esc(x.chip)} · ${esc(x.label)}</div><div class="v">${num(x.c, 0)} °C</div>${barHtml((x.c / ref) * 100, x.c > 88 ? 'crit' : x.c > 75 ? 'warn' : 'ok')}</div>`; }).join('')
        : empty('Nenhum sensor de temperatura disponível.'));
      renderGpus(s.gpus || []);
      renderPower(s.power);
    },
  });

  function renderGpus(gpus) {
    const wrap = $('#cpu-gpu-wrap');
    if (!wrap) return;
    if (!gpus.length) { wrap.innerHTML = ''; return; }
    if (!$('#gpu-tiles')) {
      wrap.innerHTML = panel('GPU', '<div class="grid g-main"><div id="gpu-chart"></div><div class="tiles" id="gpu-tiles" style="grid-template-columns:1fr"></div></div>');
    }
    chart($('#gpu-chart'), { series: [{ data: S.hist.gpu, color: 'var(--violeta)' }], ts: S.hist.ts, max: 100, height: 180 });
    const memBar = (u, tt) => (u != null && tt ? `${barHtml((u / tt) * 100, lvl((u / tt) * 100, 85, 95))}<div class="t" style="margin-top:4px">${esc(t('{u} de {t}', { u: bytes(u), t: bytes(tt) }))}</div>` : '<div class="t">—</div>');
    html('gpu-tiles', gpus.map((g) => `<div class="tile">
      <div class="t" title="${esc(g.name)}">${esc(g.vendor)} · ${esc(g.name)} <span class="muted">(${esc(g.driver || g.card)})</span></div>
      <div class="v">${g.busy_pct == null ? '—' : pct(g.busy_pct, 0)} <span class="sub" style="font-size:13px;font-weight:500">${[g.freq_mhz != null ? num(g.freq_mhz, 0) + ' MHz' : '', g.temp_c != null ? num(g.temp_c, 0) + ' °C' : '', g.power_w != null ? num(g.power_w, 1) + ' W' : ''].filter(Boolean).join(' · ')}</span></div>
      ${g.busy_pct != null ? barHtml(g.busy_pct, lvl(g.busy_pct, 80, 95)) : ''}
      <div class="t" style="margin-top:10px">VRAM</div>${memBar(g.vram_used, g.vram_total)}
      ${g.gtt_total ? `<div class="t" style="margin-top:8px">${t('GTT (memória do sistema usada pela GPU)')}</div>${memBar(g.gtt_used, g.gtt_total)}` : ''}
    </div>`).join(''));
  }

  function renderPower(pw) {
    const wrap = $('#cpu-power-wrap');
    if (!wrap) return;
    if (!pw || (!pw.batteries.length && pw.cpu_w == null && !pw.note)) { wrap.innerHTML = ''; return; }
    const st = { Charging: 'carregando', Discharging: 'descarregando', Full: 'cheia', 'Not charging': 'sem carregar' };
    const bats = pw.batteries.map((b) => `<div class="tile"><div class="t">${esc(t('Bateria {n} · {s}', { n: b.name, s: st[b.status] ? t(st[b.status]) : b.status }))}</div>
      <div class="v">${b.capacity_pct == null ? '—' : pct(b.capacity_pct, 0)}</div>${barHtml(b.capacity_pct, b.capacity_pct < 15 ? 'crit' : b.capacity_pct < 30 ? 'warn' : 'ok')}
      <div class="t" style="margin-top:6px">${esc([b.power_w != null ? num(b.power_w, 1) + ' W' : '', b.hours_left != null ? t(b.status === 'Charging' ? '{d} para carregar' : '{d} restantes', { d: dur(b.hours_left * 3600) }) : '', b.health_pct != null ? t('saúde {p}', { p: pct(b.health_pct, 0) }) : ''].filter(Boolean).join(' · '))}</div></div>`).join('');
    const cpu = pw.cpu_w != null ? `<div class="tile"><div class="t">${t('Pacote da CPU (RAPL)')}</div><div class="v">${num(pw.cpu_w, 1)} W</div></div>` : '';
    const ac = pw.ac_online == null ? '' : `<span class="chip">${t(pw.ac_online ? 'na tomada' : 'na bateria')}</span>`;
    wrap.innerHTML = panel('Energia', `<div class="tiles">${bats}${cpu}</div>${pw.note ? `<p class="muted">${esc(tErr(pw.note))}.</p>` : ''}`, `<span class="sp"></span>${ac}`);
  }

  // ---- 3. Memória
  page('memoria', 'Memória', 'mem', {
    mount(el) {
      el.innerHTML = `
        <div class="grid g4">${kpi('m-used')}${kpi('m-avail')}${kpi('m-cache')}${kpi('m-swap')}</div>
        ${panel('Composição', '<div class="stack" id="m-stack"></div><div class="legend" id="m-legend"></div>')}
        <div class="grid g-main">
          ${card('Histórico', `<div id="m-chart"></div><div class="series-legend"><span><i class="dot" style="background:var(--violeta)"></i>${t('Memória')}</span><span><i class="dot" style="background:var(--aviso)"></i>Swap</span></div>`)}
          ${card('Detalhes', '<dl class="kv" id="m-kv"></dl>')}
        </div>
        <div class="grid g2">
          ${card('Maiores consumidores de memória', '<div id="m-top"></div>')}
          ${card('Quem está na swap', '<div id="m-swap-top"></div>')}
        </div>`;
    },
    update(s) {
      const m = s.mem;
      setKpi('m-used', 'Em uso', bytes(m.used), t('{u} de {t}', { u: pct(m.pct), t: bytes(m.total) }));
      setKpi('m-avail', 'Disponível', bytes(m.available), t('livre + cache recuperável'));
      setKpi('m-cache', 'Cache', bytes(m.cache), `buffers ${bytes(m.buffers)}`);
      setKpi('m-swap', 'Swap', m.swap_total ? bytes(m.swap_used) : '—', m.swap_total ? t('{u} de {t}', { u: pct(m.swap_pct), t: bytes(m.swap_total) }) : t('swap desativada'));
      const tot = m.total || 1;
      const parts = [['Aplicativos', m.apps, 'var(--acento)'], ['Cache', m.cache, 'var(--violeta)'], ['Livre', m.free, 'transparent']];
      html('m-stack', parts.map(([, v, c]) => `<i style="width:${((v / tot) * 100).toFixed(2)}%;background:${c}"></i>`).join(''));
      html('m-legend', parts.map(([n, v, c]) => `<span><i class="dot" style="background:${c === 'transparent' ? 'var(--trilho)' : c}"></i>${esc(t(n))} <b>${bytes(v)}</b> · ${pct((v / tot) * 100, 0)}</span>`).join(''));
      chart($('#m-chart'), { series: [{ data: S.hist.mem, color: 'var(--violeta)' }, { data: S.hist.swap, color: 'var(--aviso)', fill: false }], ts: S.hist.ts, max: 100 });
      const psi = m.psi ? `${pct(m.psi.some10)} · total ${pct(m.psi.full10 ?? 0)}` : t('indisponível');
      html('m-kv', kvList([
        ['Memória compartilhada (shmem)', bytes(m.shmem)], ['Páginas sujas', bytes(m.dirty)], ['Em writeback', bytes(m.writeback)],
        ['Swap em cache', bytes(m.swap_cached)], ['Swap in / out', t('{a} / {b} pág/s', { a: num(m.swap_in_ps, 0), b: num(m.swap_out_ps, 0) })],
        ['Faltas de página maiores', num(m.major_faults_ps, 0) + '/s'], ['Swappiness', m.swappiness ?? '—'],
        ['Mortes por OOM (desde o boot)', num(m.oom_kills)], ['Pressão (PSI) 10 s', psi],
      ]));
      html('m-top', consumersTable(s.procs.top_mem, true));
      const sw = s.procs.top_swap || [];
      html('m-swap-top', sw.length ? `<div class="table-wrap"><table><thead><tr><th>${t('Processo')}</th><th class="num">${t('Na swap')}</th><th class="num">${t('Na RAM')}</th></tr></thead><tbody>
        ${sw.map((p) => `<tr><td class="trunc-s" title="${esc(p.cmd)}">${esc(p.name)} <span class="muted">· ${esc(p.pid)}</span></td><td class="num"><b>${bytes(p.swap)}</b></td><td class="num">${bytes(p.rss)}</td></tr>`).join('')}
        </tbody></table></div><p class="muted">${esc(t('Muita memória na swap com "swap in" alto deixa o sistema lento: fechar ou reiniciar esses processos libera a swap.'))}</p>`
        : empty(m.swap_total ? 'Nenhum processo legível está usando a swap.' : 'Swap desativada.'));
    },
  });

  // ---- 4. Discos
  let duPath = store.get('pulse.du') || '';
  let duTimer = 0;
  async function loadDu(path, refresh) {
    clearTimeout(duTimer);
    try {
      const q = new URLSearchParams();
      if (path) q.set('path', path);
      if (refresh) q.set('refresh', '1');
      const r = await api('/api/disk/usage?' + q);
      if (r.pending) {
        html('du-body', empty('Varrendo {p} em segundo plano… {n} entradas em {s} s. Você pode sair desta tela; o resultado fica guardado por 5 min.', { p: r.path, n: num(r.scanned), s: num(r.elapsed_s, 0) }));
        if (S.page && S.page.id === 'discos') duTimer = setTimeout(() => loadDu(r.path), 1000);
        return;
      }
      duPath = r.path;
      store.set('pulse.du', duPath);
      const parts = r.path.split('/').filter(Boolean);
      let acc = '';
      const crumbs = [`<button data-p="/">/</button>`].concat(parts.map((p) => { acc += '/' + p; return `<button data-p="${esc(acc)}">${esc(p)}</button>`; }));
      const extra = [r.truncated ? t('varredura parcial (limite de tempo/arquivos)') : '', r.errors ? t('{n} sem permissão', { n: r.errors }) : '', r.cached_s != null ? t('calculado {a}', { a: t('há {v}', { v: dur(r.cached_s) }) }) : ''].filter(Boolean);
      html('du-crumbs', crumbs.join('<span class="muted">›</span>') + `<span class="sp"></span><span class="muted">${esc([t('total {b}', { b: bytes(r.total) }), ...extra].join(' · '))}</span>`);
      const max = r.entries.length ? r.entries[0].size : 1;
      html('du-body', r.entries.length ? `<div class="table-wrap tall"><table><thead><tr><th>${t('Nome')}</th><th style="width:40%">${t('Proporção')}</th><th class="num">${t('Tamanho')}</th></tr></thead><tbody>
        ${r.entries.map((e) => `<tr class="${e.dir ? 'clickable' : ''}" ${e.dir ? `data-p="${esc(e.path)}" tabindex="0"` : ''}><td class="trunc">${e.dir ? '📁 ' : ''}${esc(e.name)}</td><td>${barHtml((e.size / (max || 1)) * 100)}</td><td class="num">${bytes(e.size)}</td></tr>`).join('')}
        </tbody></table></div>` : empty('Diretório vazio.'));
    } catch (e) {
      html('du-body', errBox(e));
    }
  }
  /** Linha da tabela SMART. `needs_root`: leitura falhou por permissão → botão "Ler com senha". */
  let smartPriv = true;
  function smartRow(d) {
    const health = d.needs_root && smartPriv
      ? `<div class="toolbar" style="flex-wrap:nowrap;gap:8px"><span class="muted trunc-s" title="${esc(tErr(d.error || ''))}">${esc(t('sem permissão'))}</span><button class="btn sm primary" data-smart-dev="${esc(d.device)}">${t('Ler com senha')}</button></div>`
      : d.error ? `<span class="muted">${esc(tErr(d.error))}</span>` : d.passed === true ? pill('aprovado', 'ok') : d.passed === false ? pill('FALHOU', 'crit') : '—';
    return `<tr data-smart-row="${esc(d.device)}"><td><b>${esc(d.device)}</b></td><td class="trunc-s">${esc(d.model || '—')}</td>
          <td>${health}</td>
          <td class="num">${d.temp_c == null ? '—' : num(d.temp_c, 0) + ' °C'}</td><td class="num">${d.power_on_hours == null ? '—' : num(d.power_on_hours)}</td>
          <td class="num">${d.reallocated == null ? '—' : num(d.reallocated)} / ${d.pending == null ? '—' : num(d.pending)}</td>
          <td class="num">${d.nvme_used_pct == null ? '—' : pct(d.nvme_used_pct, 0)} / ${d.media_errors == null ? '—' : num(d.media_errors)}</td></tr>`;
  }
  const engineName = (r) => (r.engine === 'podman' ? 'Podman' : 'Docker');
  async function smartPrivileged(btn) {
    const dev = btn.dataset.smartDev;
    const cell = btn.closest('td');
    const prev = cell.innerHTML;
    cell.innerHTML = `<span class="muted">${esc(t('Aguardando a senha…'))}</span>`;
    try {
      const r = await api('/api/disk/smart/privileged', { method: 'POST', body: { device: dev } });
      const d = (r && r.disk) || (r && Array.isArray(r.disks) && r.disks[0]) || r;
      const row = $(`#d-smart tr[data-smart-row="${CSS.escape(dev)}"]`);
      if (row && d && d.device) row.outerHTML = smartRow(d);
      else cell.innerHTML = prev;
    } catch (e) {
      cell.innerHTML = prev;
      toast(t('Falhou: {m}', { m: e.message }), true);
    }
  }
  async function loadSmart() {
    html('d-smart', empty('Consultando o smartctl (≈ 1 s por disco)…'));
    try {
      const r = await api('/api/disk/smart');
      if (!r.available) {
        html('d-smart', `<p class="sub">${esc(tErr(r.reason))}. ${t('Para habilitar:')} <code class="cmd">${esc(r.install)}</code></p><p class="muted">${t('Ler SMART exige root ou o grupo {g}; o Pulse só pede a senha quando você clica em “Ler com senha”.', { g: '<code class="cmd">disk</code>' })}</p>`);
        return;
      }
      html('d-smart', `<div class="table-wrap"><table><thead><tr><th>${t('Disco')}</th><th>${t('Modelo')}</th><th>${t('Saúde')}</th><th class="num">${t('Temperatura')}</th><th class="num">${t('Horas ligado')}</th><th class="num">${t('Setores realocados / pendentes')}</th><th class="num">${t('NVMe: desgaste / erros')}</th></tr></thead><tbody>
        ${(smartPriv = r.privileged !== false, r.disks.map(smartRow).join(''))}
        </tbody></table></div>`);
    } catch (e) { html('d-smart', errBox(e)); }
  }
  page('discos', 'Discos', 'disk', {
    mount(el) {
      el.innerHTML = `
        ${card('Sistemas de arquivos', '<div id="d-fs"></div>')}
        <div class="grid g-main">
          ${card('Throughput', `<div id="d-chart"></div><div class="series-legend"><span><i class="dot" style="background:var(--acento)"></i>${t('Leitura')}</span><span><i class="dot" style="background:var(--violeta)"></i>${t('Escrita')}</span></div>`)}
          ${card('Pressão de E/S', '<dl class="kv" id="d-kv"></dl>')}
        </div>
        ${card('Dispositivos', '<div id="d-dev"></div>')}
        ${card('Saúde dos discos (SMART)', `<div id="d-smart"><p class="sub">${t('Leitura sob demanda via {c}.', { c: '<code class="cmd">smartctl</code>' })}</p></div>`, `<span class="sp"></span><button class="btn sm" id="d-smart-go">${t('Verificar')}</button>`)}
        ${panel('O que ocupa espaço?', '<div class="crumbs toolbar" id="du-crumbs"></div><div id="du-body" style="margin-top:10px"></div>', `<span class="sp"></span><button class="btn sm" id="du-home">${t('Pasta pessoal')}</button><button class="btn sm" id="du-reload">${t('Atualizar')}</button>`)}
        ${panel('Candidatos à limpeza', `<p class="sub">${t('O Pulse <b>nunca executa</b> estes comandos — só sugere. Revise antes de rodar no terminal.')}</p><div id="d-clean">${empty('Calculando tamanhos…')}</div>`)}`;
      $('#d-smart').addEventListener('click', (e) => { const b = e.target.closest('button[data-smart-dev]'); if (b) smartPrivileged(b); });
      const nav = (e) => { const t = e.target.closest('[data-p]'); if (t) loadDu(t.dataset.p); };
      $('#du-crumbs').addEventListener('click', nav);
      $('#du-body').addEventListener('click', nav);
      $('#du-body').addEventListener('keydown', (e) => { if (e.key === 'Enter') nav(e); });
      $('#du-home').addEventListener('click', () => loadDu(''));
      $('#du-reload').addEventListener('click', () => loadDu(duPath, true));
      $('#d-smart-go').addEventListener('click', loadSmart);
      loadDu(duPath);
      api('/api/disk/cleanup').then((list) => {
        html('d-clean', list.length ? `<div class="table-wrap"><table><thead><tr><th>${t('Item')}</th><th>${t('Caminho')}</th><th class="num">${t('Tamanho')}</th><th>${t('Comando sugerido')}</th></tr></thead><tbody>
          ${list.map((c) => `<tr><td><b>${esc(tErr(c.title))}</b><div class="muted" style="font-size:12px">${esc(tErr(c.description))}</div></td><td class="mono trunc-s">${esc(c.path)}</td><td class="num">${c.size == null ? '—' : bytes(c.size)}</td><td><code class="cmd">${esc(c.command)}</code></td></tr>`).join('')}
          </tbody></table></div>` : empty('Nada a sugerir.'));
      }).catch((e) => html('d-clean', errBox(e)));
    },
    unmount() { clearTimeout(duTimer); },
    update(s) {
      html('d-fs', `<div class="table-wrap"><table><thead><tr><th>${t('Montagem')}</th><th>${t('Dispositivo')}</th><th>${t('Tipo')}</th><th style="width:28%">${t('Uso')}</th><th class="num">${t('Usado / total')}</th><th class="num">${t('Livre')}</th><th class="num">Inodes</th></tr></thead><tbody>
        ${s.disks.fs.map((f) => `<tr><td><b>${esc(f.mount)}</b></td><td class="mono">${esc(f.device)}</td><td>${esc(f.fstype)}</td><td>${barHtml(f.pct, lvl(f.pct, 90, 97))}</td><td class="num">${bytes(f.used)} / ${bytes(f.total)} · ${pct(f.pct, 0)}</td><td class="num">${bytes(f.avail)}</td><td class="num">${pct(f.inodes_pct, 0)}</td></tr>`).join('')}
        </tbody></table></div>`);
      chart($('#d-chart'), { series: [{ data: S.hist.rd, color: 'var(--acento)' }, { data: S.hist.wr, color: 'var(--violeta)' }], ts: S.hist.ts, max: 'auto', fmt: (v) => rate(v) });
      const p = s.disks.psi;
      html('d-kv', kvList([['Leitura agora', rate(s.disks.read_bps)], ['Escrita agora', rate(s.disks.write_bps)],
        ['PSI some (10 s)', p ? pct(p.some10) : '—'], ['PSI full (10 s)', p && p.full10 != null ? pct(p.full10) : '—'],
        ['PSI some (60 s)', p ? pct(p.some60) : '—']]));
      html('d-dev', s.disks.devices.length ? `<div class="table-wrap"><table><thead><tr><th>${t('Dispositivo')}</th><th>${t('Modelo')}</th><th class="num">${t('Tamanho')}</th><th class="num">${t('Leitura')}</th><th class="num">${t('Escrita')}</th><th class="num">${t('IOPS (l/e)')}</th><th style="width:20%">${t('Ocupação')}</th></tr></thead><tbody>
        ${s.disks.devices.map((d) => `<tr><td><b>${esc(d.name)}</b> <span class="muted">${d.rotational ? 'HDD' : 'SSD'}</span></td><td class="trunc-s">${esc(d.model || '—')}</td><td class="num">${bytes(d.size)}</td><td class="num">${rate(d.read_bps)}</td><td class="num">${rate(d.write_bps)}</td><td class="num">${num(d.read_iops, 0)} / ${num(d.write_iops, 0)}</td><td><div class="toolbar" style="flex-wrap:nowrap"><div style="flex:1">${barHtml(d.busy_pct, lvl(d.busy_pct, 70, 90))}</div><span class="num sub">${pct(d.busy_pct, 0)}</span></div></td></tr>`).join('')}
        </tbody></table></div>` : empty('Nenhum dispositivo de bloco.'));
    },
  });

  // ---- 5. Processos
  const P = { list: [], sort: { k: 'cpu', dir: -1 }, filter: '', mine: false, timer: 0, view: store.get('pulse.pview') || 'lista' };
  const STATE_NAMES = { R: 'rodando', S: 'dormindo', D: 'E/S', I: 'ocioso', Z: 'zumbi', T: 'parado', t: 'rastreado', X: 'morto' };
  async function loadProcs() {
    clearTimeout(P.timer);
    try {
      const r = await api('/api/processes');
      P.list = r.list;
      const s = r.summary;
      html('p-summary', [['Total', s.total], ['Rodando', s.running], ['Dormindo', s.sleeping], ['Zumbis', s.zombie], ['Parados', s.stopped], ['Threads', s.threads]]
        .map(([k, v]) => `<span class="chip">${esc(t(k))} <b>${num(v)}</b></span>`).join(''));
      renderProcs();
    } catch { /* tenta de novo */ }
    if (S.page && S.page.id === 'processos') P.timer = setTimeout(loadProcs, 2000);
  }
  const ioCell = (p) => (p.io_rd == null ? `<span class="muted" title="${esc(t('sem permissão para /proc/PID/io'))}">—</span>` : `${rate(p.io_rd)} / ${rate(p.io_wr)}`);
  const procActions = (p) => `<div class="actions" data-pid="${p.pid}" data-name="${esc(p.name)}">
          <button class="btn sm" data-a="${p.state === 'T' ? 'resume' : 'pause'}">${t(p.state === 'T' ? 'Continuar' : 'Pausar')}</button>
          <button class="btn sm" data-a="renice" data-nice="${p.nice}">${t('Prioridade')}</button>
          <button class="btn sm danger" data-a="terminate">${t('Encerrar')}</button></div>`;
  function renderTree() {
    const f = P.filter.toLowerCase();
    const byPid = new Map(P.list.map((p) => [p.pid, p]));
    const kids = new Map();
    for (const p of P.list) { if (!kids.has(p.ppid)) kids.set(p.ppid, []); kids.get(p.ppid).push(p); }
    // totais da subárvore
    const tot = new Map();
    const sum = (p, depth = 0) => {
      if (tot.has(p.pid)) return tot.get(p.pid);
      let c = p.cpu, r = p.rss;
      if (depth < 64) for (const k of kids.get(p.pid) || []) { const t = sum(k, depth + 1); c += t[0]; r += t[1]; }
      const t = [c, r]; tot.set(p.pid, t); return t;
    };
    const match = (p) => (!f || p.cmd.toLowerCase().includes(f) || p.name.toLowerCase().includes(f) || String(p.pid) === f || p.user.toLowerCase() === f) && (!P.mine || !S.user || p.user === S.user);
    // com filtro: mostra os que casam e seus ancestrais
    let keep = null;
    if (f || P.mine) {
      keep = new Set();
      for (const p of P.list) if (match(p)) { let q = p, g = 0; while (q && !keep.has(q.pid) && g++ < 64) { keep.add(q.pid); q = byPid.get(q.ppid); } }
    }
    const roots = P.list.filter((p) => !byPid.has(p.ppid));
    const rows = [];
    const walk = (p, depth) => {
      if (rows.length >= 400 || (keep && !keep.has(p.pid))) return;
      rows.push([p, depth]);
      const ch = (kids.get(p.pid) || []).slice().sort((a, b) => sum(b)[0] - sum(a)[0] || sum(b)[1] - sum(a)[1]);
      for (const k of ch) walk(k, depth + 1);
    };
    roots.sort((a, b) => sum(b)[0] - sum(a)[0]).forEach((r) => walk(r, 0));
    $('#p-count').textContent = t('{n} processos na árvore', { n: rows.length }) + (rows.length >= 400 ? ' ' + t('(limite de 400)') : '');
    html('p-table', `<table class="t-compact"><thead><tr><th>${t('Processo')}</th><th class="num">PID</th><th>${t('Usuário')}</th><th class="num">CPU</th><th class="num">${t('CPU (árvore)')}</th><th class="num" title="${esc(t('Soma do RSS da subárvore; memória compartilhada entra mais de uma vez'))}">${t('RSS somado')}</th><th class="num">${t('Ações')}</th></tr></thead><tbody>
      ${rows.map(([p, d]) => { const t = sum(p); const n = (kids.get(p.pid) || []).length; return `<tr><td class="trunc" title="${esc(p.cmd)}" style="padding-left:${10 + d * 18}px">${d ? '<span class="muted">└ </span>' : ''}<b>${esc(p.name)}</b>${n ? ` <span class="muted">(${n})</span>` : ''}</td>
        <td class="num">${p.pid}</td><td>${esc(p.user)}</td><td class="num">${pct(p.cpu)}</td><td class="num"><b>${pct(t[0])}</b></td><td class="num">${bytes(t[1])}</td><td>${procActions(p)}</td></tr>`; }).join('')}
      </tbody></table><p class="muted">${esc(t('RSS somado conta memória compartilhada (bibliotecas, abas do mesmo navegador) mais de uma vez; use como ordem de grandeza.'))}</p>`);
  }
  function renderProcs() {
    if (P.view === 'arvore') { renderTree(); return; }
    const f = P.filter.toLowerCase();
    let rows = P.list.filter((p) => !f || p.cmd.toLowerCase().includes(f) || p.name.toLowerCase().includes(f) || String(p.pid) === f || p.user.toLowerCase() === f);
    if (P.mine && S.user) rows = rows.filter((p) => p.user === S.user);
    const { k, dir } = P.sort;
    const key = (p) => (k === 'io' ? (p.io_rd ?? -1) + (p.io_wr ?? 0) : p[k]);
    rows.sort((a, b) => (key(a) < key(b) ? -dir : key(a) > key(b) ? dir : 0));
    const total = rows.length;
    rows = rows.slice(0, 300);
    $('#p-count').textContent = total > 300 ? t('mostrando 300 de {n}', { n: total }) : t('{n} processos', { n: total });
    const cols = [['pid', 'PID', 1], ['user', 'Usuário'], ['state', 'Estado'], ['cpu', 'CPU', 1], ['rss', 'RAM', 1], ['io', 'Disco L/E', 1], ['threads', 'Threads', 1], ['nice', 'Nice', 1], ['cmd', 'Comando']];
    html('p-table', `<table class="t-compact"><thead><tr>${cols.map(([c, l, n]) => `<th class="sortable ${n ? 'num' : ''}" data-k="${c}" aria-sort="${P.sort.k === c ? (dir > 0 ? 'ascending' : 'descending') : 'none'}">${esc(t(l))}</th>`).join('')}<th class="num">${t('Ações')}</th></tr></thead><tbody>
      ${rows.map((p) => `<tr><td class="num">${p.pid}</td><td>${esc(p.user)}</td><td>${STATE_NAMES[p.state] ? pill(STATE_NAMES[p.state], p.state === 'R' ? 'ok' : p.state === 'Z' ? 'crit' : p.state === 'T' ? 'warn' : '') : rawPill(p.state)}</td>
        <td class="num">${pct(p.cpu)}</td><td class="num">${bytes(p.rss)}</td><td class="num">${ioCell(p)}</td><td class="num">${p.threads}</td><td class="num">${p.nice}</td>
        <td class="trunc p-cmd" title="${esc(p.cmd)}"><b>${esc(p.name)}</b> <span class="muted">${esc(p.cmd)}</span></td>
        <td>${procActions(p)}</td></tr>`).join('')}
      </tbody></table>`);
  }
  async function procAction(pid, name, action, curNice) {
    const label = `<b>${esc(name)}</b> (PID ${esc(pid)})`;
    let body;
    const audit = `<p class="muted">${t('A ação é registrada no log de auditoria.')}</p>`;
    if (action === 'terminate') {
      if (!(await confirmAction(t('Encerrar processo?'), `<p>${t('Enviar SIGTERM para {p}? Se não encerrar em 5 s, o Pulse envia SIGKILL.', { p: label })}</p>${audit}`, t('Encerrar')))) return;
    } else if (action === 'pause') {
      if (!(await confirmAction(t('Pausar processo?'), `<p>${t('Enviar SIGSTOP para {p}? Ele fica congelado até ser continuado.', { p: label })}</p>`, t('Pausar'), 'primary'))) return;
    } else if (action === 'resume') {
      if (!(await confirmAction(t('Continuar processo?'), `<p>${t('Enviar SIGCONT para {p}?', { p: label })}</p>`, t('Continuar'), 'primary'))) return;
    } else if (action === 'renice') {
      const v = await dialog({
        title: t('Ajustar prioridade'),
        body: `<p>${t('Novo valor de <i>nice</i> para {p} (−20 = maior prioridade, 19 = menor). Diminuir o nice exige privilégios.', { p: label })}</p>
          <label class="field">Nice<input id="nice" class="input" type="number" min="-20" max="19" step="1" value="${esc(curNice ?? 0)}"></label>`,
        actions: [{ label: t('Cancelar'), value: null }, { label: t('Aplicar'), cls: 'primary', onclick: () => closeDialog($('#nice').value) }],
      });
      if (v == null || v === '') return;
      body = { nice: Number(v) };
    }
    try {
      const r = await api(`/api/processes/${encodeURIComponent(pid)}/${action}`, { method: 'POST', body: body || {} });
      toast(r.message ? tErr(r.message) : t('Feito.'));
    } catch (e) { toast(t('Falhou: {m}', { m: e.message }), true); }
    if (S.page && S.page.id === 'processos') loadProcs();
    if (S.page && S.page.id === 'agentes') pollAgents();
  }
  document.addEventListener('click', (e) => {
    const b = e.target.closest('.actions[data-pid] button[data-a]');
    if (!b) return;
    const box = b.parentElement;
    procAction(box.dataset.pid, box.dataset.name, b.dataset.a, b.dataset.nice);
  });
  page('processos', 'Processos', 'list', {
    mount(el) {
      el.innerHTML = `
        <div class="toolbar" id="p-summary"></div>
        ${panel('', `<div class="toolbar" style="margin-bottom:12px">
            <input id="p-filter" class="input" type="search" placeholder="${esc(t('Filtrar por nome, comando, PID ou usuário'))}" style="flex:1;max-width:420px" value="${esc(P.filter)}">
            <label class="chk"><input type="checkbox" id="p-mine" ${P.mine ? 'checked' : ''}> ${t('só meus')}</label>
            <span class="seg" id="p-view"><button data-v="lista" aria-pressed="${P.view === 'lista'}">${t('Lista')}</button><button data-v="arvore" aria-pressed="${P.view === 'arvore'}">${t('Árvore')}</button></span>
            <span class="sp"></span><span class="muted" id="p-count"></span></div>
          <div class="table-wrap tall" id="p-table">${empty('Carregando…')}</div>`)}`;
      $('#p-filter').addEventListener('input', (e) => { P.filter = e.target.value; renderProcs(); });
      $('#p-mine').addEventListener('change', (e) => { P.mine = e.target.checked; renderProcs(); });
      $('#p-view').addEventListener('click', (e) => { const b = e.target.closest('button'); if (!b) return; P.view = b.dataset.v; store.set('pulse.pview', P.view); $$('#p-view button').forEach((x) => x.setAttribute('aria-pressed', String(x === b))); renderProcs(); });
      $('#p-table').addEventListener('click', (e) => {
        const th = e.target.closest('th[data-k]');
        if (!th) return;
        const k = th.dataset.k;
        P.sort = { k, dir: P.sort.k === k ? -P.sort.dir : (k === 'cpu' || k === 'rss' || k === 'threads' || k === 'io' ? -1 : 1) };
        renderProcs();
      });
      loadProcs();
    },
    unmount() { clearTimeout(P.timer); },
  });

  // ---- 6. Serviços
  const SV = { scope: 'user', filter: 'todos', q: '', units: [], timer: 0, cg: new Map(), ct: [] };
  async function loadCgroups() {
    try {
      const r = await api('/api/cgroups');
      SV.cg = new Map(r.units.map((u) => [u.scope + ':' + u.unit, u]));
      SV.ctUsage = r.units.filter((u) => u.container);
    } catch { /* opcional */ }
  }
  async function loadServices() {
    clearTimeout(SV.timer);
    try {
      const [r] = await Promise.all([api('/api/services?scope=' + SV.scope), loadCgroups()]);
      SV.units = r.units;
      renderServices();
    } catch (e) { html('sv-table', errBox(e)); }
    if (S.page && S.page.id === 'servicos') SV.timer = setTimeout(loadServices, 10000);
  }
  function renderServices() {
    const q = SV.q.toLowerCase();
    const rows = SV.units.filter((u) => {
      if (SV.filter === 'ativos' && u.active !== 'active') return false;
      if (SV.filter === 'falhas' && u.active !== 'failed') return false;
      if (SV.filter === 'inativos' && u.active !== 'inactive') return false;
      return !q || u.name.toLowerCase().includes(q) || u.description.toLowerCase().includes(q);
    });
    const counts = { todos: SV.units.length, ativos: 0, falhas: 0, inativos: 0 };
    for (const u of SV.units) { if (u.active === 'active') counts.ativos++; else if (u.active === 'failed') counts.falhas++; else if (u.active === 'inactive') counts.inativos++; }
    $$('#sv-filter button').forEach((b) => { b.textContent = `${t(b.dataset.label)} (${counts[b.dataset.f]})`; });
    const st = (u) => (u.active === 'active' ? (u.sub === 'running' ? pill('rodando', 'ok') : rawPill(u.sub, 'ok')) : u.active === 'failed' ? pill('falhou', 'crit') : u.active === 'activating' ? pill('iniciando', 'info') : u.active === 'inactive' ? pill('inativo') : rawPill(u.active));
    html('sv-table', rows.length ? `<table class="t-compact"><thead><tr><th>${t('Unidade')}</th><th>${t('Descrição')}</th><th>${t('Estado')}</th><th>${t('Habilitado')}</th><th class="num">CPU</th><th class="num">${t('Memória')}</th><th class="num">${t('Ações')}</th></tr></thead><tbody>
      ${rows.map((u) => { const cg = SV.cg.get(SV.scope + ':' + u.name); return `<tr><td class="trunc-s" title="${esc(u.name)}"><b>${esc(u.name.replace(/\.service$/, ''))}</b></td><td class="trunc">${esc(u.description)}</td><td>${st(u)}</td><td>${esc(u.enabled || '—')}</td>
        <td class="num">${cg && cg.cpu != null ? pct(cg.cpu) : '—'}</td><td class="num">${cg && cg.mem ? bytes(cg.mem) : '—'}</td>
        <td><div class="actions" data-unit="${esc(u.name)}">
          <button class="btn sm ghost" data-v="logs">Logs</button>
          ${u.active === 'active' ? `<button class="btn sm" data-v="restart">${t('Reiniciar')}</button><button class="btn sm danger" data-v="stop">${t('Parar')}</button>` : `<button class="btn sm primary" data-v="start">${t('Iniciar')}</button>`}
        </div></td></tr>`; }).join('')}</tbody></table>` : empty('Nenhum serviço corresponde ao filtro.'));
  }
  const fmtWhen = (ms) => (ms ? `${dateTime(ms)} (${ms > Date.now() ? t('em {d}', { d: dur((ms - Date.now()) / 1000) }) : ago(ms)})` : '—');
  async function loadTimers() {
    try {
      const r = await api('/api/timers?scope=' + SV.scope);
      html('sv-timers', r.timers.length ? `<div class="table-wrap"><table><thead><tr><th>Timer</th><th>${t('Ativa')}</th><th>${t('Próxima execução')}</th><th>${t('Última execução')}</th></tr></thead><tbody>
        ${r.timers.map((x) => `<tr><td><b>${esc(x.unit.replace(/\.timer$/, ''))}</b></td><td class="trunc-s">${esc(x.activates)}</td><td>${esc(fmtWhen(x.next))}</td><td>${esc(fmtWhen(x.last))}</td></tr>`).join('')}</tbody></table></div>` : empty('Nenhum timer neste escopo.'));
    } catch (e) { html('sv-timers', errBox(e)); }
  }
  async function loadCron() {
    try {
      const r = await api('/api/cron');
      const PER = { hourly: 'a cada hora', daily: 'diário', weekly: 'semanal', monthly: 'mensal' };
      const per = r.periodic.filter((p) => p.scripts.length).map((p) => `<span class="chip">${esc(PER[p.period] ? t(PER[p.period]) : p.period)}: <b>${esc(p.scripts.join(', '))}</b></span>`).join(' ');
      html('sv-cron', (r.entries.length ? `<div class="table-wrap"><table><thead><tr><th>${t('Agenda')}</th><th>${t('Usuário')}</th><th>${t('Comando')}</th><th>${t('Origem')}</th></tr></thead><tbody>
        ${r.entries.map((c) => `<tr><td class="mono">${esc(c.schedule)}</td><td>${esc(c.user)}</td><td class="mono trunc" title="${esc(c.command)}">${esc(c.command)}</td><td class="muted trunc-s">${esc(c.source)}</td></tr>`).join('')}</tbody></table></div>` : empty('Nenhuma entrada de cron.'))
        + (per ? `<div class="toolbar" style="margin-top:10px">${per}</div>` : '') + `<p class="muted">${esc(t('Variáveis de ambiente do crontab não são exibidas e valores que parecem senhas/tokens são mascarados.'))}</p>`);
    } catch (e) { html('sv-cron', errBox(e)); }
  }
  const ctUse = (c) => (SV.ctUsage || []).find((u) => u.container.startsWith(c.id));
  async function containerLogs(id, name) {
    let r;
    try { r = await api('/api/containers/logs?lines=300&id=' + encodeURIComponent(id)); } catch (e) { toast(t('Logs: {m}', { m: e.message }), true); return; }
    const done = dialog({
      title: t('Logs de {n}', { n: name }),
      wide: true,
      body: `<p class="muted">${t('Últimas 300 linhas (stdout + stderr, horário UTC).')}</p><div class="logs" id="ct-logs" style="max-height:60vh;white-space:pre-wrap">${esc(r.text) || `<span class="muted">${t('sem saída')}</span>`}</div>`,
      actions: [{ label: t('Fechar'), cls: 'primary', value: true }],
    });
    const box = $('#ct-logs');
    if (box) box.scrollTop = box.scrollHeight;
    await done;
  }
  async function loadContainers(stats) {
    try {
      if (!SV.ctUsage) await loadCgroups();
      const r = await api('/api/containers' + (stats ? '?stats=1' : ''));
      if (!r.available) { html('sv-ct', empty('{e} indisponível: {m}', { e: engineName(r), m: tErr(r.error || '') })); return; }
      const st = (c) => (c.state === 'running' ? pill('rodando', 'ok') : c.state === 'exited' ? pill('parado') : c.state === 'paused' ? pill('pausado', 'warn') : c.state === 'restarting' ? pill('reiniciando', 'info') : rawPill(c.state, c.state === 'dead' ? 'crit' : ''));
      html('sv-ct', r.containers.length ? `<p class="sub">${esc(t('{e} {v} · {a} de {n} em execução', { e: engineName(r), v: r.version, a: r.containers.filter((c) => c.state === 'running').length, n: r.containers.length }))}</p><div class="table-wrap"><table class="t-compact"><thead><tr><th>${t('Contêiner')}</th><th>${t('Imagem')}</th><th>${t('Estado')}</th><th>Status</th><th>${t('Portas')}</th><th class="num">CPU</th><th class="num">${t('Memória')}</th><th class="num">${t('Ações')}</th></tr></thead><tbody>
        ${r.containers.map((c) => `<tr><td class="trunc-s" title="${esc(c.name)}"><b>${esc(c.name)}</b><div class="muted mono" style="font-size:11.5px">${esc(c.id)}${c.compose ? ' · ' + esc(c.compose) : ''}</div></td><td class="trunc-s">${esc(c.image)}</td><td>${st(c)}</td><td class="trunc-s">${esc(c.status)}</td><td class="mono trunc-s" title="${esc(c.ports)}">${esc(c.ports || '—')}</td>
          <td class="num">${c.cpu != null ? pct(c.cpu) : ctUse(c) && ctUse(c).cpu != null ? pct(ctUse(c).cpu) : '—'}</td><td class="num">${c.mem ? esc(c.mem) : ctUse(c) ? bytes(ctUse(c).mem) : '—'}</td>
          <td><div class="actions" data-ct="${esc(c.id)}" data-ctname="${esc(c.name)}"><button class="btn sm ghost" data-c="logs">Logs</button>${c.state === 'running' ? `<button class="btn sm" data-c="restart">${t('Reiniciar')}</button><button class="btn sm danger" data-c="stop">${t('Parar')}</button>` : `<button class="btn sm primary" data-c="start">${t('Iniciar')}</button>`}</div></td></tr>`).join('')}
        </tbody></table></div>` : empty('Nenhum contêiner.'));
    } catch (e) { html('sv-ct', errBox(e)); }
  }
  page('servicos', 'Serviços', 'gear', {
    mount(el) {
      el.innerHTML = panel('', `<div class="toolbar" style="margin-bottom:12px">
          <span class="seg" id="sv-scope"><button data-s="user" aria-pressed="${SV.scope === 'user'}">${t('Usuário')}</button><button data-s="system" aria-pressed="${SV.scope === 'system'}">${t('Sistema')}</button></span>
          <span class="seg" id="sv-filter">${[['todos', 'Todos'], ['ativos', 'Ativos'], ['falhas', 'Falhas'], ['inativos', 'Inativos']].map(([f, l]) => `<button data-f="${f}" data-label="${l}" aria-pressed="${SV.filter === f}">${t(l)}</button>`).join('')}</span>
          <input id="sv-q" class="input" type="search" placeholder="${esc(t('Buscar serviço'))}" value="${esc(SV.q)}">
          <span class="sp"></span><button class="btn sm" id="sv-reload">${t('Atualizar')}</button></div>
        <p class="sub" id="sv-note"></p>
        <div class="table-wrap tall" id="sv-table">${empty('Carregando…')}</div>`)
        + panel('Timers do systemd', `<div id="sv-timers">${empty('Carregando…')}</div>`, '<span class="sp"></span><span class="muted" id="sv-timers-scope" style="text-transform:none;letter-spacing:0;font-weight:500"></span>')
        + panel('Cron', `<div id="sv-cron">${empty('Carregando…')}</div>`)
        + panel('Contêineres (Docker/Podman)', `<div id="sv-ct">${empty('Carregando…')}</div>`, `<span class="sp"></span><button class="btn sm" id="sv-ct-stats">${t('Medir CPU/memória')}</button><button class="btn sm" id="sv-ct-reload">${t('Atualizar')}</button>`);
      const note = () => { $('#sv-timers-scope').textContent = t(SV.scope === 'user' ? 'escopo: usuário' : 'escopo: sistema'); loadTimers();  $('#sv-note').innerHTML = SV.scope === 'system' ? t('Ações no escopo do <b>sistema</b> abrem a janela de senha do polkit (helper do Pulse via {c}, sem sudo); a autorização vale por alguns minutos. Sem agente polkit na sessão, a ação falha com aviso.', { c: '<code class="cmd">pkexec</code>' }) : t('Serviços do seu usuário ({c}).', { c: '<code class="cmd">systemctl --user</code>' }); };
      note();
      $('#sv-scope').addEventListener('click', (e) => { const b = e.target.closest('button'); if (!b) return; SV.scope = b.dataset.s; $$('#sv-scope button').forEach((x) => x.setAttribute('aria-pressed', String(x === b))); note(); html('sv-table', empty('Carregando…')); loadServices(); });
      $('#sv-filter').addEventListener('click', (e) => { const b = e.target.closest('button'); if (!b) return; SV.filter = b.dataset.f; $$('#sv-filter button').forEach((x) => x.setAttribute('aria-pressed', String(x === b))); renderServices(); });
      $('#sv-q').addEventListener('input', (e) => { SV.q = e.target.value; renderServices(); });
      $('#sv-reload').addEventListener('click', loadServices);
      $('#sv-table').addEventListener('click', async (e) => {
        const b = e.target.closest('button[data-v]');
        if (!b) return;
        const unit = b.parentElement.dataset.unit, v = b.dataset.v;
        if (v === 'logs') {
          LG.scope = SV.scope; LG.unit = unit; LG.prio = ''; LG.q = ''; LG.cursor = '';
          location.hash = '#/logs';
          return;
        }
        const verb = t({ start: 'Iniciar', stop: 'Parar', restart: 'Reiniciar' }[v]);
        const q = t({ start: 'Iniciar serviço?', stop: 'Parar serviço?', restart: 'Reiniciar serviço?' }[v]);
        if (!(await confirmAction(q, `<p>${t('{v} {u} (escopo {s})?', { v: verb, u: `<b>${esc(unit)}</b>`, s: t(SV.scope === 'user' ? 'usuário' : 'sistema') })}</p><p class="muted">${t('A ação é registrada no log de auditoria.')}</p>`, verb, v === 'stop' ? 'danger' : 'primary'))) return;
        try { await api('/api/services/action', { method: 'POST', body: { scope: SV.scope, unit, action: v } }); toast(`${verb}: ${unit} — ok`); } catch (err) { toast(t('Falhou: {m}', { m: err.message }), true); }
        loadServices();
      });
      $('#sv-ct-reload').addEventListener('click', async () => { await loadCgroups(); loadContainers(false); });
      $('#sv-ct-stats').addEventListener('click', () => { html('sv-ct', empty('Medindo (docker stats leva alguns segundos)…')); loadContainers(true); });
      $('#sv-ct').addEventListener('click', async (e) => {
        const b = e.target.closest('button[data-c]');
        if (!b) return;
        const box = b.parentElement, v = b.dataset.c;
        if (v === 'logs') { containerLogs(box.dataset.ct, box.dataset.ctname); return; }
        const verb = t({ start: 'Iniciar', stop: 'Parar', restart: 'Reiniciar' }[v]);
        const q = t({ start: 'Iniciar contêiner?', stop: 'Parar contêiner?', restart: 'Reiniciar contêiner?' }[v]);
        if (!(await confirmAction(q, `<p>${verb} <b>${esc(box.dataset.ctname)}</b>?</p><p class="muted">${t('A ação é registrada no log de auditoria.')}</p>`, verb, v === 'stop' ? 'danger' : 'primary'))) return;
        try { await api('/api/containers/action', { method: 'POST', body: { id: box.dataset.ct, action: v } }); toast(`${verb}: ${box.dataset.ctname} — ok`); } catch (err) { toast(t('Falhou: {m}', { m: err.message }), true); }
        loadContainers(false);
      });
      loadServices();
      loadCron();
      loadContainers(false);
    },
    unmount() { clearTimeout(SV.timer); },
  });

  // ---- 7. Rede
  const NP = { last: null, busy: false };
  function renderProbe(r) {
    const disk = r.disk_ops ? t('{n} operações de disco · latência média {a} ms · máxima {m} ms', { n: num(r.disk_ops), a: num(r.disk_avg_us / 1000, 2), m: num(r.disk_max_us / 1000, 1) }) : t('nenhuma operação de disco no intervalo');
    html('n-probe', `<p class="muted">${esc(t('Últimos {s} s · {d}', { s: r.seconds, d: disk }))}</p>` + (r.procs.length ? `<div class="table-wrap"><table class="t-compact"><thead><tr><th>${t('Processo')}</th><th class="num">PID</th><th class="num">${t('↓ média')}</th><th class="num">${t('↑ média')}</th><th class="num">${t('Recebido')}</th><th class="num">${t('Enviado')}</th></tr></thead><tbody>
      ${r.procs.map((p) => `<tr><td><b>${esc(p.comm)}</b></td><td class="num">${p.pid}</td><td class="num">${rate(p.rx_bps)}</td><td class="num">${rate(p.tx_bps)}</td><td class="num">${bytes(p.rx)}</td><td class="num">${bytes(p.tx)}</td></tr>`).join('')}</tbody></table></div>` : empty('Nenhum tráfego TCP/UDP no intervalo.')));
  }
  async function probeNet() {
    if (NP.busy) return;
    const b = $('#n-probe-go');
    const secs = Number($('#n-probe-s').value) || 10;
    NP.busy = true;
    b.disabled = true;
    b.textContent = t('Medindo…');
    html('n-probe', empty('Aguardando a senha e medindo por {s} s…', { s: secs }));
    try {
      NP.last = await api('/api/probe/net', { method: 'POST', body: { seconds: secs } });
      if ($('#n-probe')) renderProbe(NP.last);
    } catch (e) {
      if ($('#n-probe')) html('n-probe', errBox(e));
    } finally {
      NP.busy = false;
      const bb = $('#n-probe-go');
      if (bb) { bb.disabled = false; bb.textContent = t('Medir'); }
    }
  }
  page('rede', 'Rede', 'globe', {
    mount(el) {
      el.innerHTML = `
        <div class="grid g3">${kpi('n-rx')}${kpi('n-tx')}${kpi('n-tcp')}</div>
        ${card('Tráfego', '<div id="n-chart"></div><div class="series-legend"><span><i class="dot" style="background:var(--acento)"></i>Download</span><span><i class="dot" style="background:var(--violeta)"></i>Upload</span></div>')}
        ${card('Interfaces', '<div id="n-if"></div>')}
        ${card('Tráfego por processo (eBPF)', `<p class="sub">${t('Medição sob demanda com bpftrace: bytes enviados/recebidos por processo e latência dos discos no intervalo. Pede sua senha (polkit) e precisa do pacote {p}.', { p: '<code class="cmd">bpftrace</code>' })}</p><div id="n-probe"></div>`, `<span class="sp"></span><select id="n-probe-s" class="input" style="width:auto" aria-label="${esc(t('Duração'))}"><option value="5">5 s</option><option value="10" selected>10 s</option><option value="30">30 s</option></select><button class="btn sm primary" id="n-probe-go">${t('Medir')}</button>`)}
        ${card('Portas em escuta', '<div id="n-listen"></div>', `<span class="sp"></span><span class="muted" style="text-transform:none;letter-spacing:0;font-weight:500">${t('processos de outros usuários aparecem sem dono')}</span>`)}`;
      $('#n-probe-go').addEventListener('click', probeNet);
      if (NP.last) renderProbe(NP.last);
    },
    update(s) {
      const n = s.net;
      setKpi('n-rx', 'Download', '↓ ' + rate(n.rx_bps), t('todas as interfaces físicas'));
      setKpi('n-tx', 'Upload', '↑ ' + rate(n.tx_bps), t('todas as interfaces físicas'));
      const tcp = n.tcp || {};
      const others = Object.entries(tcp).filter(([k]) => k !== 'ESTABLISHED' && k !== 'LISTEN').map(([k, v]) => `${k.toLowerCase().replace('_', ' ')} ${v}`).join(' · ');
      setKpi('n-tcp', 'Conexões TCP', num(tcp.ESTABLISHED || 0), t('estabelecidas · escutando {n}', { n: tcp.LISTEN || 0 }) + (others ? ' · ' + others : ''));
      chart($('#n-chart'), { series: [{ data: S.hist.rx, color: 'var(--acento)' }, { data: S.hist.tx, color: 'var(--violeta)' }], ts: S.hist.ts, max: 'auto', fmt: (v) => rate(v) });
      html('n-if', `<div class="table-wrap"><table><thead><tr><th>Interface</th><th>${t('Estado')}</th><th class="num">${t('Velocidade')}</th><th class="num">${t('↓ agora')}</th><th class="num">${t('↑ agora')}</th><th class="num">${t('Recebido')}</th><th class="num">${t('Enviado')}</th><th class="num">${t('Erros/descartes')}</th></tr></thead><tbody>
        ${n.ifaces.map((i) => `<tr><td><b>${esc(i.name)}</b> ${i.virtual ? `<span class="muted">${t('virtual')}</span>` : ''}<div class="muted mono" style="font-size:11.5px">${esc(i.mac)}</div></td><td>${i.state === 'up' ? pill('ativa', 'ok') : i.state === 'unknown' ? pill('desconhecido') : i.state === 'down' ? pill('inativa', 'warn') : rawPill(i.state)}</td>
          <td class="num">${i.speed_mbps ? num(i.speed_mbps) + ' Mb/s' : '—'}</td><td class="num">${rate(i.rx_bps)}</td><td class="num">${rate(i.tx_bps)}</td><td class="num">${bytes(i.rx_bytes)}</td><td class="num">${bytes(i.tx_bytes)}</td><td class="num">${num(i.rx_errs + i.tx_errs)}</td></tr>`).join('')}
        </tbody></table></div>`);
      html('n-listen', n.listening.length ? `<div class="table-wrap tall"><table><thead><tr><th>Proto</th><th>${t('Endereço')}</th><th class="num">${t('Porta')}</th><th>${t('Processo')}</th><th class="num">PID</th><th>${t('Alcance')}</th></tr></thead><tbody>
        ${n.listening.map((l) => `<tr><td>${esc(l.proto)}</td><td class="mono">${esc(l.addr)}</td><td class="num"><b>${l.port}</b></td><td>${esc(l.process || '—')}</td><td class="num">${l.pid ?? '—'}</td><td>${l.scope === 'local' ? pill('só local', 'ok') : pill('acessível na rede', 'warn')}</td></tr>`).join('')}
        </tbody></table></div>` : empty('Nenhuma porta em escuta.'));
    },
  });

  // ---- 8. Logs
  const LG = { scope: store.get('pulse.logscope') || 'system', unit: '', prio: '', lines: '200', q: '', follow: false, cursor: '', timer: 0, rows: [] };
  const PRIO = ['emergência', 'alerta', 'crítico', 'erro', 'aviso', 'notícia', 'info', 'debug'];
  function logRow(l) {
    const d = new Date(l.ts);
    const when = d.toLocaleDateString(LOC(), { day: '2-digit', month: '2-digit' }) + ' ' + d.toLocaleTimeString(LOC());
    return `<div class="log p${l.priority}"><span title="${esc(dateTime(l.ts))}">${esc(when)}</span><span class="u" title="${esc(l.unit || l.ident)}">${esc(l.ident || l.unit || '—')}${l.pid ? '[' + esc(l.pid) + ']' : ''}</span><span class="m">${esc(l.message)}</span></div>`;
  }
  async function loadLogs(append) {
    clearTimeout(LG.timer);
    const p = new URLSearchParams({ scope: LG.scope, lines: LG.lines });
    if (LG.unit) p.set('unit', LG.unit);
    if (LG.prio !== '') p.set('priority', LG.prio);
    if (LG.q) p.set('q', LG.q);
    if (append && LG.cursor) p.set('cursor', LG.cursor);
    try {
      const r = await api('/api/logs?' + p);
      if (r.cursor) LG.cursor = r.cursor;
      const box = $('#lg-box');
      if (!box) return;
      if (append) {
        if (r.lines.length) {
          const atBottom = box.scrollHeight - box.scrollTop - box.clientHeight < 40;
          box.insertAdjacentHTML('beforeend', r.lines.map(logRow).join(''));
          while (box.children.length > 2000) box.firstChild.remove();
          if (atBottom) box.scrollTop = box.scrollHeight;
        }
      } else {
        box.innerHTML = r.lines.length ? r.lines.map(logRow).join('') : `<p class="empty">${esc(r.note ? tErr(r.note) : t('Nenhuma linha.'))}</p>`;
        box.scrollTop = box.scrollHeight;
      }
      $('#lg-count').textContent = t('{n} linhas', { n: box.querySelectorAll('.log').length });
    } catch (e) { html('lg-box', errBox(e)); }
    if (LG.follow && S.page && S.page.id === 'logs') LG.timer = setTimeout(() => loadLogs(true), 2000);
  }
  page('logs', 'Logs', 'doc', {
    mount(el) {
      el.innerHTML = panel('', `<div class="toolbar" style="margin-bottom:12px">
          <span class="seg" id="lg-scope"><button data-s="system" aria-pressed="${LG.scope === 'system'}">${t('Sistema')}</button><button data-s="user" aria-pressed="${LG.scope === 'user'}">${t('Usuário')}</button></span>
          <input id="lg-unit" class="input" placeholder="${esc(t('unidade (ex.: ssh.service)'))}" aria-label="${esc(t('Unidade'))}" value="${esc(LG.unit)}" style="width:200px">
          <select id="lg-prio" class="input" aria-label="${esc(t('Prioridade'))}"><option value="">${t('todas as prioridades')}</option>${PRIO.map((n, i) => `<option value="${i}" ${String(i) === LG.prio ? 'selected' : ''}>${esc(t('até {i} · {n}', { i, n: t(n) }))}</option>`).join('')}</select>
          <select id="lg-lines" class="input" aria-label="${esc(t('Linhas'))}">${['100', '200', '500', '1000'].map((n) => `<option ${n === LG.lines ? 'selected' : ''}>${n}</option>`).join('')}</select>
          <input id="lg-q" class="input" type="search" placeholder="${esc(t('Buscar texto'))}" value="${esc(LG.q)}" style="flex:1;min-width:160px">
          <label class="chk"><input type="checkbox" id="lg-follow" ${LG.follow ? 'checked' : ''}> ${t('seguir')}</label>
          <button class="btn sm primary" id="lg-go">${t('Atualizar')}</button></div>
        <div class="toolbar" style="margin-bottom:8px">${pill('erro+', 'crit')}${pill('aviso', 'warn')}${pill('notícia', 'info')}${pill('info')}<span class="sp"></span><span class="muted" id="lg-count"></span></div>
        <div class="logs" id="lg-box">${empty('Carregando…')}</div>`);
      const reload = () => { LG.cursor = ''; loadLogs(false); };
      $('#lg-scope').addEventListener('click', (e) => { const b = e.target.closest('button'); if (!b) return; LG.scope = b.dataset.s; store.set('pulse.logscope', LG.scope); $$('#lg-scope button').forEach((x) => x.setAttribute('aria-pressed', String(x === b))); reload(); });
      $('#lg-unit').addEventListener('change', (e) => { LG.unit = e.target.value.trim(); reload(); });
      $('#lg-prio').addEventListener('change', (e) => { LG.prio = e.target.value; reload(); });
      $('#lg-lines').addEventListener('change', (e) => { LG.lines = e.target.value; reload(); });
      $('#lg-q').addEventListener('change', (e) => { LG.q = e.target.value.trim(); reload(); });
      $('#lg-follow').addEventListener('change', (e) => { LG.follow = e.target.checked; if (LG.follow) loadLogs(true); else clearTimeout(LG.timer); });
      $('#lg-go').addEventListener('click', reload);
      reload();
    },
    unmount() { clearTimeout(LG.timer); },
  });

  // ---- 9. Agentes
  let openSession = null;
  const fmtMs = (ms) => (ms == null ? '—' : ms < 1000 ? `${num(ms, 0)} ms` : ms < 60000 ? `${num(ms / 1000, 1)} s` : dur(ms / 1000));
  const usd = (v) => { const p = LANG === 'en' ? '$' : 'US$ '; return v == null ? '—' : v < 0.01 && v > 0 ? '< ' + p + num(0.01, 2) : p + num(v, 2); };
  /** Preço de um modelo: nome exato ou o prefixo mais longo cadastrado. */
  function priceFor(model) {
    const prices = (S.agents && S.agents.prefs && S.agents.prefs.prices) || {};
    if (!model) return null;
    if (prices[model]) return prices[model];
    let best = null;
    for (const k of Object.keys(prices)) if (model.startsWith(k) && (!best || k.length > best.length)) best = k;
    return best ? prices[best] : null;
  }
  const costOf = (model, tin, tout, cr, cw) => {
    const p = priceFor(model);
    return p ? (tin * p[0] + tout * p[1] + cr * p[2] + cw * p[3]) / 1e6 : null;
  };
  const sessionCost = (s) => costOf(s.model, s.tokens_in, s.tokens_out, s.tokens_cache_read || 0, s.tokens_cache_write || 0);
  /** Estado de uma sessão: hook mais recente ou inferido pela atividade da transcrição. */
  function sessionState(s, h) {
    const recent = Date.now() - s.updated < 30e3;
    if (h && h.ts >= s.updated - 5e3) return statePill(h.state);
    if (recent) return pill(h ? 'executando (inferido)' : 'ativa', 'ok');
    if (h) return statePill(h.state);
    return Date.now() - s.updated < 120e3 ? pill('ativa', 'ok') : pill('inativa');
  }

  function renderAgentsPage() {
    const A = S.agents;
    if (!A || !$('#ag-kpis')) return;
    if (!A.enabled) { html('ag-body', empty('Detecção de agentes desativada (--no-agents).')); return; }
    const ps = A.processes, ss = A.sessions;
    const mcp = ps.flatMap((p) => p.mcp);
    const active = ss.filter((s) => Date.now() - s.updated < 5 * 60e3).length;
    const day24 = ss.filter((s) => Date.now() - s.updated < 86400e3);
    setKpi('ag-k1', 'Processos de agentes', String(ps.length), [...new Set(ps.map((p) => p.agent))].join(', ') || t('nenhum'));
    setKpi('ag-k2', 'Servidores MCP', String(mcp.length), [...new Set(mcp.map((m) => m.name))].join(', ') || t('nenhum detectado'));
    setKpi('ag-k3', 'Sessões ativas / recentes', `${active} / ${day24.length}`, t('ativas: últimos 5 min · recentes: 24 h'));
    // tokens e custo de hoje a partir do índice diário
    const rows = (A.usage && A.usage.rows) || [];
    const today = rows.filter((r) => r.day === A.usage.today);
    const tTok = today.reduce((a, r) => a + r.input + r.output + r.cache_read + r.cache_write, 0);
    const tCosts = today.map((r) => costOf(r.model, r.input, r.output, r.cache_read, r.cache_write));
    const priced = tCosts.filter((c) => c != null);
    setKpi('ag-k4', 'Tokens hoje', kilo(tTok), priced.length ? t('custo estimado {c}', { c: usd(priced.reduce((a, c) => a + c, 0)) }) + (priced.length < tCosts.length ? ' ' + t('(parcial: há modelos sem preço)') : '') : t('cadastre preços em Preferências para ver o custo'));

    const hs = A.hooks.sessions;
    const sessById = Object.fromEntries(ss.map((s) => [s.id, s]));
    const live = hs.slice(0, 6).map((h) => {
      const s = sessById[h.session];
      const inferred = s && s.updated > h.ts + 5e3 && Date.now() - s.updated < 30e3;
      return `<div class="agent-card"><div class="hd"><strong>${esc(agentName(h.agent))}</strong><span class="sp"></span>${inferred ? pill('executando (inferido)', 'ok') : statePill(h.state)}</div>
        <div class="meta">${esc(t('sessão {id}', { id: h.session.slice(0, 8) }))} · ${ago(Math.max(h.ts, s ? s.updated : 0))}${h.cwd ? ' · ' + esc(h.cwd.split('/').pop()) : ''}</div>
        <div class="toolbar">${h.event ? rawPill(h.event, h.event === 'PostToolUseFailure' ? 'crit' : h.event === 'PermissionRequest' ? 'warn' : 'info') : ''}<span class="sub trunc-s" title="${esc(h.tool || h.message || '')}">${esc(h.tool || h.message || '')}</span></div>
        ${h.subagents || h.failures ? `<div class="toolbar" style="margin-top:6px">${h.subagents ? pill(h.subagents > 1 ? t('{n} subagentes rodando', { n: h.subagents }) : t('1 subagente rodando'), 'vio') : ''}${h.failures ? `<span title="${esc(t('última: {m}', { m: h.last_failure }))}">${pill(h.failures > 1 ? t('{n} falhas de ferramenta', { n: h.failures }) : t('1 falha de ferramenta'), 'crit')}</span>` : ''}</div>` : ''}</div>`;
    });
    // Codex sem hook recente mas com transcrição ativa: "executando (inferido)"
    const hookIds = new Set(hs.map((h) => h.session));
    for (const s of ss) {
      if (live.length >= 6) break;
      if (s.agent === 'Codex' && !hookIds.has(s.id) && Date.now() - s.updated < 30e3) {
        live.push(`<div class="agent-card"><div class="hd"><strong>Codex</strong><span class="sp"></span>${pill('executando (inferido)', 'ok')}</div>
          <div class="meta">${esc(t('sessão {id}', { id: s.id.slice(0, 8) }))} · ${ago(s.updated)} · ${esc(s.project_name || '')}</div><p class="muted" style="margin:4px 0 0">${esc(t('Inferido pela transcrição em atividade (o notify do Codex só avisa no fim do turno).'))}</p></div>`);
      }
    }
    html('ag-live', live.length ? `<div class="grid g3">${live.join('')}</div>`
      : empty('Nenhum evento de hook recebido ainda. Use “Conectar hooks” para o Claude Code enviar estados ao vivo.'));

    html('ag-procs', ps.length ? `<div class="grid g2">${ps.map((p) => `<div class="agent-card"><div class="hd"><strong>${esc(p.agent)}</strong><span class="muted">${esc(p.name)}</span><span class="sp"></span>${p.state === 'T' ? pill('pausado', 'warn') : pill('ativo', 'ok')}</div>
        <div class="meta"><span>PID <b>${p.pid}</b></span><span>CPU <b>${pct(p.tree_cpu)}</b></span><span>RAM <b>${bytes(p.tree_rss)}</b></span><span>${esc(p.children === 1 ? t('1 filho') : t('{n} filhos', { n: p.children }))}</span></div>
        <div class="meta mono trunc" title="${esc(p.cwd || '')}">${esc(p.cwd || '—')}</div>
        ${p.mcp.length ? `<div class="meta">MCP: ${p.mcp.map((m) => rawPill(m.name, 'vio')).join(' ')}</div>` : ''}
        <div class="actions" data-pid="${p.pid}" data-name="${esc(p.agent)}" style="justify-content:flex-start;margin-top:8px">
          <button class="btn sm" data-a="${p.state === 'T' ? 'resume' : 'pause'}">${t(p.state === 'T' ? 'Continuar' : 'Pausar')}</button>
          <button class="btn sm danger" data-a="terminate">${t('Encerrar')}</button></div></div>`).join('')}</div>`
      : empty('Nenhum processo de agente em execução.'));

    const hookBySession = Object.fromEntries(hs.map((h) => [h.session, h]));
    const note = A.openclaw_note ? `<p class="sub">${esc(A.openclaw_note)}.</p>` : '';
    html('ag-sessions', ss.length ? `${note}<div class="table-wrap"><table class="t-compact"><thead><tr><th>${t('Agente')}</th><th>${t('Projeto')}</th><th>${t('Estado')}</th><th>${t('Modelo')}</th><th class="num" title="${esc(t('pedidos do usuário / chamadas de ferramentas'))}">${t('Pedidos / ferr.')}</th><th class="num">${t('Ativo / esperando você')}</th><th class="num">${t('Tokens in/out')}</th><th class="num">${t('Custo')}</th><th class="num">${t('Atividade')}</th></tr></thead><tbody>
      ${ss.map((s) => { const tools = s.tools.reduce((a, x) => a + x.n, 0); return `<tr class="clickable" data-sid="${esc(s.id)}" tabindex="0" aria-expanded="${openSession === s.id}">
        <td style="white-space:nowrap"><b>${esc(s.agent)}</b>${s.subagent_msgs ? `<div class="muted" style="font-size:11.5px">${esc(t('{n} msgs de subagentes', { n: s.subagent_msgs }))}</div>` : ''}</td>
        <td><div class="trunc-s"><b>${esc(s.title && s.agent.startsWith('OpenClaw') ? s.title : s.project_name || '—')}</b></div><div class="muted mono trunc-s" style="font-size:11.5px">${esc(s.project)}</div></td>
        <td>${sessionState(s, hookBySession[s.id])}</td><td class="trunc-s" title="${esc(s.model)}">${esc(s.model || '—')}</td>
        <td class="num">${s.agent.startsWith('OpenClaw') ? esc(t('{n} msgs', { n: s.messages })) : `${s.prompts} / ${tools}`}</td>
        <td class="num">${s.active_ms || s.wait_ms ? `${fmtMs(s.active_ms)} / ${fmtMs(s.wait_ms)}` : '—'}</td>
        <td class="num">${kilo(s.tokens_in + s.tokens_cache)} / ${kilo(s.tokens_out)}</td><td class="num">${usd(sessionCost(s))}</td><td class="num">${ago(s.updated)}</td></tr>`; }).join('')}
      </tbody></table></div><div id="ag-timeline"></div>` : `${note}${empty('Nenhuma sessão recente em ~/.claude/projects, ~/.codex/sessions ou no OpenClaw.')}`);
    renderTimeline();
    renderUsage();

    const ev = A.hooks.events;
    html('ag-events', ev.length ? ev.slice(0, 30).map((e) => `<div class="toolbar" style="margin-bottom:6px"><span class="muted" style="width:70px">${ago(e.ts)}</span><b>${esc(agentName(e.agent))}</b>${statePill(e.state)}${e.event ? rawPill(e.event, e.event === 'PostToolUseFailure' ? 'crit' : e.event === 'PermissionRequest' ? 'warn' : 'info') : ''}<span class="sub trunc" title="${esc(e.tool || e.message)}">${esc(e.tool || e.message)}</span></div>`).join('') : empty('Sem eventos.'));
  }

  function renderTimeline() {
    const el = $('#ag-timeline');
    if (!el) return;
    const s = S.agents && S.agents.sessions.find((x) => x.id === openSession);
    if (!s) { el.innerHTML = ''; return; }
    const kinds = { prompt: ['var(--texto-3)', 'Pedido'], tool: ['var(--acento)', ''], reply: ['var(--violeta)', 'Resposta'], done: ['var(--ok)', 'Concluído'] };
    const timed = s.tools.filter((t) => t.avg_ms != null).sort((a, b) => b.avg_ms * b.n - a.avg_ms * a.n);
    const details = S.agents.prefs && S.agents.prefs.show_tool_details;
    const c = sessionCost(s);
    el.innerHTML = `<div class="grid g-main" style="margin-top:12px;grid-template-columns:minmax(0,1.3fr) minmax(0,1fr)">
      <div><div class="toolbar"><b>${t('Linha do tempo')}</b><span class="muted trunc">${esc(s.title || s.id)}</span></div>
        <ol class="timeline">${s.timeline.slice().reverse().map((x) => { const [cl, l] = kinds[x.kind] || ['var(--texto-3)', x.kind]; return `<li><i class="dot" style="background:${cl}"></i><time>${esc(clock(x.ts))}</time><span class="trunc">${esc(kinds[x.kind] ? t(l || 'Ferramenta') : l)}${x.kind === 'tool' ? ' · <b title="' + esc(x.label) + '">' + esc(x.label) + '</b>' : ''}</span></li>`; }).join('') || `<li class="muted">${t('sem eventos')}</li>`}</ol></div>
      <div><dl class="kv">
        <dt>${t('Duração')}</dt><dd>${s.started ? dur((s.updated - s.started) / 1000) : '—'}</dd>
        <dt>${t('Tempo ativo')}</dt><dd>${fmtMs(s.active_ms)}</dd>
        <dt>${t('Esperando você')}</dt><dd>${fmtMs(s.wait_ms)}</dd>
        <dt>${t('Tokens: entrada / saída')}</dt><dd>${kilo(s.tokens_in)} / ${kilo(s.tokens_out)}</dd>
        <dt>${t('Cache: leitura / escrita')}</dt><dd>${kilo(s.tokens_cache_read || 0)} / ${kilo(s.tokens_cache_write || 0)}</dd>
        <dt>${t('Custo estimado')}</dt><dd>${c == null ? (s.model ? esc(t('sem preço para {m}', { m: s.model })) : '—') : usd(c)}</dd></dl>
        ${timed.length ? `<div class="table-wrap" style="margin-top:12px"><table class="t-compact"><thead><tr><th>${t('Ferramenta')}</th><th class="num">${t('Chamadas')}</th><th class="num">${t('Média')}</th><th class="num">${t('Máx.')}</th></tr></thead><tbody>
          ${timed.slice(0, 8).map((x) => `<tr><td class="trunc-s" title="${esc(x.name)}">${esc(x.name)}</td><td class="num">${x.n}</td><td class="num">${fmtMs(x.avg_ms)}</td><td class="num">${fmtMs(x.max_ms)}</td></tr>`).join('')}</tbody></table></div>` : ''}
      </div></div>
      ${s.partial ? `<p class="muted">${t('Transcrição grande: só o trecho final foi lido.')}</p>` : ''}
      <p class="muted">${esc((details ? t('Modo detalhado ligado: a linha do tempo mostra comandos e caminhos das ferramentas (mascarados).') + ' ' : '') + t('O texto dos pedidos e respostas nunca é exibido. "Esperando você" = do fim de uma resposta até o próximo pedido.'))}</p>`;
  }

  function renderUsage() {
    const el = $('#ag-usage');
    if (!el || !S.agents) return;
    const U = S.agents.usage || { rows: [] };
    if (!U.rows.length) { el.innerHTML = empty('Ainda sem dados (o índice dos últimos 30 dias é montado em segundo plano a cada 5 min).'); return; }
    // agrega por dia e agente
    const days = [];
    const end = new Date(U.today + 'T12:00:00Z');
    for (let i = 29; i >= 0; i--) days.push(new Date(end - i * 86400e3).toISOString().slice(0, 10));
    const agents = [...new Set(U.rows.map((r) => r.agent))];
    const colors = { 'Claude Code': 'var(--acento)', Codex: 'var(--violeta)' };
    const per = Object.fromEntries(days.map((d) => [d, {}]));
    const perModel = {};
    for (const r of U.rows) {
      const tk = r.input + r.output + r.cache_read + r.cache_write;
      if (!tk) continue;
      if (per[r.day]) per[r.day][r.agent] = (per[r.day][r.agent] || 0) + tk;
      const m = (perModel[r.agent + '|' + r.model] ||= { agent: r.agent, model: r.model, input: 0, output: 0, cache_read: 0, cache_write: 0, cost: 0, priced: true });
      m.input += r.input; m.output += r.output; m.cache_read += r.cache_read; m.cache_write += r.cache_write;
      const c = costOf(r.model, r.input, r.output, r.cache_read, r.cache_write);
      if (c == null) m.priced = false; else m.cost += c;
    }
    const max = Math.max(1, ...days.map((d) => agents.reduce((a, g) => a + (per[d][g] || 0), 0)));
    const bars = days.map((d) => {
      const segs = agents.map((g) => { const v = per[d][g] || 0; return v ? `<i title="${esc(g)}: ${kilo(v)} tokens" style="display:block;height:${(v / max) * 100}%;background:${colors[g] || 'var(--aviso)'}"></i>` : ''; }).join('');
      return `<div title="${esc(d)}" style="flex:1;display:flex;flex-direction:column-reverse;height:140px;background:var(--trilho);border-radius:4px;overflow:hidden">${segs}</div>`;
    }).join('');
    const models = Object.values(perModel).sort((a, b) => (b.input + b.output + b.cache_read + b.cache_write) - (a.input + a.output + a.cache_read + a.cache_write));
    const totalCost = models.filter((m) => m.priced).reduce((a, m) => a + m.cost, 0);
    el.innerHTML = `<div style="display:flex;gap:3px;align-items:flex-end">${bars}</div>
      <div class="toolbar" style="margin:6px 0 12px"><span class="muted">${esc(days[0])}</span><span class="sp"></span><div class="series-legend">${agents.map((g) => `<span><i class="dot" style="background:${colors[g] || 'var(--aviso)'}"></i>${esc(g)}</span>`).join('')}</div><span class="sp"></span><span class="muted">${t('hoje')}</span></div>
      <div class="table-wrap"><table><thead><tr><th>${t('Agente')}</th><th>${t('Modelo')}</th><th class="num">${t('Entrada')}</th><th class="num">${t('Saída')}</th><th class="num">${t('Cache (leitura / escrita)')}</th><th class="num">${t('Custo estimado')}</th></tr></thead><tbody>
        ${models.map((m) => `<tr><td>${esc(m.agent)}</td><td class="trunc-s" title="${esc(m.model)}">${esc(m.model)}</td><td class="num">${kilo(m.input)}</td><td class="num">${kilo(m.output)}</td><td class="num">${kilo(m.cache_read)} / ${kilo(m.cache_write)}</td><td class="num">${m.priced ? usd(m.cost) : `<span class="muted">${t('sem preço')}</span>`}</td></tr>`).join('')}
      </tbody></table></div>
      <p class="muted">${t('Total com preço cadastrado em 30 dias: {c}. Custos são estimativas a partir da tabela de preços que você cadastra (US$ por milhão de tokens); assinaturas com franquia não cobram por token.', { c: `<b>${usd(totalCost)}</b>` })}</p>`;
  }

  async function prefsDialog() {
    let cfg;
    try { cfg = await api('/api/agents'); } catch (e) { toast(e.message, true); return; }
    const pr = cfg.prefs || { prices: {} };
    const models = [...new Set([...(cfg.usage ? cfg.usage.rows.map((r) => r.model) : []), ...cfg.sessions.map((s) => s.model)].filter((m) => m && m !== '?'))].sort();
    const row = (m, p) => `<tr data-row><td><input class="input mono" data-k="model" value="${esc(m)}" placeholder="${esc(t('modelo'))}" aria-label="${esc(t('Modelo'))}" list="pulse-models" style="width:100%"></td>${[0, 1, 2, 3].map((i) => `<td><input class="input" type="number" min="0" step="any" data-k="p${i}" value="${p ? esc(p[i]) : ''}" style="width:90px"></td>`).join('')}<td><button class="btn sm ghost" data-del aria-label="${esc(t('Remover'))}">✕</button></td></tr>`;
    const rows = Object.entries(pr.prices || {}).map(([m, p]) => row(m, p)).join('');
    const done = dialog({
      title: t('Preferências dos agentes'),
      wide: true,
      body: `<label class="chk" style="margin:6px 0"><input type="checkbox" id="pf-notify" ${pr.notify_attention ? 'checked' : ''}> ${t('Avisar no desktop quando um agente pedir atenção (permissão, pergunta) — no máximo 1 aviso por minuto por sessão')}</label><br>
        <label class="chk" style="margin:6px 0"><input type="checkbox" id="pf-details" ${pr.show_tool_details ? 'checked' : ''}> ${t('Modo detalhado: mostrar comandos e caminhos das ferramentas na linha do tempo e nos eventos ao vivo (valores que parecem senhas/tokens são mascarados; o texto das conversas nunca aparece)')}</label>
        <h3 style="margin:16px 0 6px;font-size:14px">${t('Preços (US$ por milhão de tokens)')}</h3>
        <p class="muted" style="margin-top:0">${t('O Pulse não traz preços prontos: confira os valores atuais no site do provedor. Um modelo casa pelo nome exato ou pelo prefixo mais longo (ex.: {c} vale para todas as versões).', { c: '<code class="cmd">claude-opus</code>' })}</p>
        <datalist id="pulse-models">${models.map((m) => `<option value="${esc(m)}">`).join('')}</datalist>
        <div class="table-wrap"><table id="pf-prices"><thead><tr><th>${t('Modelo')}</th><th>${t('Entrada')}</th><th>${t('Saída')}</th><th>${t('Cache: leitura')}</th><th>${t('Cache: escrita')}</th><th></th></tr></thead><tbody>${rows}</tbody></table></div>
        <button class="btn sm" id="pf-add" style="margin-top:8px">${t('Adicionar modelo')}</button>`,
      actions: [{ label: t('Cancelar'), value: null }, { label: t('Salvar'), cls: 'primary', onclick: () => {
        const prices = {};
        for (const tr of $$('#pf-prices tr[data-row]')) {
          const m = tr.querySelector('[data-k="model"]').value.trim();
          if (!m) continue;
          prices[m] = [0, 1, 2, 3].map((i) => Number(tr.querySelector(`[data-k="p${i}"]`).value || 0));
        }
        closeDialog({ notify_attention: $('#pf-notify').checked, show_tool_details: $('#pf-details').checked, prices });
      } }],
    });
    $('#pf-add').addEventListener('click', () => $('#pf-prices tbody').insertAdjacentHTML('beforeend', row('', null)));
    $('#pf-prices').addEventListener('click', (e) => { const b = e.target.closest('[data-del]'); if (b) b.closest('tr').remove(); });
    const body = await done;
    if (!body) return;
    try { await api('/api/config/agents', { method: 'POST', body }); toast(t('Preferências salvas.')); } catch (e) { toast(t('Não salvou: {m}', { m: e.message }), true); }
    pollAgents();
  }

  async function hooksDialog() {
    let info;
    try { info = await api('/api/agents/hooks'); } catch (e) { toast(e.message, true); return; }
    const conn = info.connected.length;
    const codexOk = !!info.codex_connected;
    const code = (s) => `<code class="cmd">${s}</code>`;
    const done = dialog({
      title: t('Conectar hooks'),
      body: `<p>${t('Os hooks fazem o Claude Code avisar o Pulse quando está <b>executando</b>, <b>pedindo atenção</b> (inclusive pedidos de permissão), <b>aguardando</b> ou <b>concluído</b>, além de subagentes e falhas de ferramentas.')}</p>
        <dl class="kv"><dt>${t('Arquivo')}</dt><dd class="mono trunc-s" title="${esc(info.settings)}">${esc(info.settings)}</dd>
        <dt>Script</dt><dd class="mono trunc-s" title="${esc(info.script)}">${esc(info.script)}</dd>
        <dt>${t('Script instalado')}</dt><dd>${info.script_exists ? pill('sim', 'ok') : pill('não — rode install.sh', 'warn')}</dd>
        <dt>${t('Eventos conectados')}</dt><dd>${esc(t('{a} de {b}', { a: conn, b: info.total_events }))}</dd></dl>
        <p class="sub">${t('Instalar adiciona os eventos que faltam ao {a}, preservando o que já existe e criando um backup {b}:', { a: code('settings.json'), b: code('settings.json.pulse-bak') })}</p>
        <pre class="snippet mono">${esc(info.snippet)}</pre>
        <div class="toolbar" style="margin:12px 0 4px"><b>Codex</b>${codexOk ? pill('conectado', 'ok') : pill('não conectado')}<span class="sp"></span>${codexOk ? '' : `<button class="btn sm primary" id="hk-codex">${t('Conectar Codex')}</button>`}</div>
        ${codexOk ? '' : `<p class="sub">${t('“Conectar Codex” grava o {n} no início de {f} (com backup). Se já existir outro {n} lá, nada é alterado e o motivo aparece num aviso. Alternativa: adicione você mesmo, antes de qualquer {t}:', { n: code('notify'), f: code('~/.codex/config.toml'), t: code(t('[tabela]')) })}</p>
        <pre class="snippet mono">${esc(info.codex_snippet)}</pre>`}
        <p class="muted">${t('OpenClaw: chame {c} a partir das notificações dele.', { c: code(`pulse-hook &lt;${esc(t('estado'))}&gt; openclaw`) })}</p>`,
      actions: [{ label: t('Fechar'), value: false }, { label: t(conn === info.total_events ? 'Já conectado' : 'Instalar no Claude Code'), cls: 'primary', value: conn !== info.total_events }],
    });
    const cb = $('#hk-codex');
    if (cb) cb.addEventListener('click', async () => {
      cb.disabled = true;
      try {
        const r = await api('/api/agents/hooks/install', { method: 'POST', body: { agent: 'codex' } });
        toast(r && r.message ? tErr(r.message) : r && r.backup ? t('Codex conectado · backup em {b}', { b: r.backup }) : t('Codex conectado.'));
        closeDialog(false);
        hooksDialog();
      } catch (e) {
        cb.disabled = false;
        toast(t('Falhou: {m}', { m: e.message }), true);
      }
    });
    const ok = await done;
    if (!ok) return;
    try {
      const r = await api('/api/agents/hooks/install', { method: 'POST', body: {} });
      toast(r.added.length ? t('Hooks adicionados: {h}', { h: r.added.join(', ') }) + (r.backup ? ' · ' + t('backup em {b}', { b: r.backup }) : '') : t('Os hooks já estavam conectados.'));
    } catch (e) { toast(t('Falhou: {m}', { m: e.message }), true); }
  }
  page('agentes', 'Agentes', 'bot', {
    mount(el) {
      el.innerHTML = `
        <div class="toolbar"><p class="lead">${t('Processos, sessões e eventos dos agentes de IA rodando nesta máquina.')}</p><span class="sp"></span><button class="btn" id="ag-prefs">${t('Preferências')}</button><button class="btn" id="ag-hooks">${t('Conectar hooks')}</button></div>
        <div id="ag-body" style="display:contents">
        <div class="grid g4" id="ag-kpis">${kpi('ag-k1')}${kpi('ag-k2')}${kpi('ag-k3')}${kpi('ag-k4')}</div>
        ${panel('Ao vivo (hooks)', '<div id="ag-live"></div>')}
        ${panel('Processos', '<div id="ag-procs"></div>')}
        ${panel('Sessões', '<div id="ag-sessions"></div>')}
        ${panel('Uso por dia (30 dias)', '<div id="ag-usage"></div>')}
        ${panel('Eventos recentes (hooks)', '<div id="ag-events"></div>')}</div>`;
      $('#ag-hooks').addEventListener('click', hooksDialog);
      $('#ag-prefs').addEventListener('click', prefsDialog);
      const toggle = (e) => {
        const tr = e.target.closest('tr[data-sid]');
        if (!tr) return;
        openSession = openSession === tr.dataset.sid ? null : tr.dataset.sid;
        $$('#ag-sessions tr[data-sid]').forEach((r) => r.setAttribute('aria-expanded', String(r.dataset.sid === openSession)));
        renderTimeline();
      };
      $('#ag-sessions').addEventListener('click', toggle);
      $('#ag-sessions').addEventListener('keydown', (e) => { if (e.key === 'Enter') toggle(e); });
      renderAgentsPage();
      pollAgents();
    },
  });

  // ---- 10. Alertas
  let alertsTimer = 0;
  async function loadAlerts() {
    clearTimeout(alertsTimer);
    try {
      const r = await api('/api/alerts');
      html('al-active', r.active.length ? r.active.map((a) => `<div class="toolbar" style="margin-bottom:10px">${sevPill(a.severity)}<b>${esc(tErr(a.title))}</b><span class="sub">${esc(tErr(a.detail))}</span><span class="sp"></span><span class="muted">${esc(t('desde {h} ({a})', { h: clock(a.since), a: ago(a.since) }))}</span></div>`).join('') : empty('Tudo certo — nenhum alerta ativo.'));
      S.ruleValues = r.values;
      html('al-rules', `<table><thead><tr><th>${t('Regra')}</th><th>${t('Condição')}</th><th>${t('Severidade')}</th></tr></thead><tbody>${r.rules.map((x) => `<tr><td><b>${esc(tErr(x.title))}</b></td><td>${esc(tErr(x.condition))}</td><td>${sevPill(x.severity)}</td></tr>`).join('')}</tbody></table>`);
      html('al-hist', r.history.length ? `<div class="table-wrap tall"><table><thead><tr><th>${t('Quando')}</th><th>${t('Evento')}</th><th>${t('Alerta')}</th><th>${t('Detalhe')}</th></tr></thead><tbody>${r.history.map((h) => `<tr><td class="num">${esc(dateTime(h.ts))}</td><td>${h.kind === 'resolvido' ? pill('resolvido', 'ok') : h.kind === 'agravou' ? pill('agravou', 'crit') : pill('disparou', 'warn')}</td><td><b>${esc(tErr(h.alert.title))}</b></td><td>${esc(tErr(h.alert.detail))}</td></tr>`).join('')}</tbody></table></div>` : empty('Nenhum alerta registrado ainda.'));
      html('al-notify', r.notify ? `${pill('ativada', 'ok')} <span class="sub">${t('Alertas novos vão para o desktop via {c}.', { c: '<code class="cmd">notify-send</code>' })}</span>` : `${pill('desativada')} <span class="sub">${t('Inicie o Pulse com {c} para receber notificações no desktop.', { c: '<code class="cmd">--notify</code>' })}</span>`);
    } catch { /* tenta de novo */ }
    if (S.page && S.page.id === 'alertas') alertsTimer = setTimeout(loadAlerts, 3000);
  }
  const RULE_FIELDS = [
    ['cpu_pct', 'CPU alta (%)'], ['cpu_samples', '…por N amostras'], ['mem_warn', 'Memória: aviso (%)'], ['mem_crit', 'Memória: crítico (%)'],
    ['swap_warn', 'Swap: aviso (%)'], ['disk_warn', 'Disco: aviso (%)'], ['disk_crit', 'Disco: crítico (%)'], ['temp_crit', 'Temperatura (°C)'],
    ['proc_pct', 'Processo preso (% CPU)'], ['proc_samples', '…por N amostras'],
  ];
  async function editRules() {
    let cfg;
    try { cfg = await api('/api/config'); } catch (e) { toast(e.message, true); return; }
    const v = cfg.alerts;
    const ok = await dialog({
      title: t('Editar limiares de alerta'),
      body: `<p class="sub">${t('Grava a seção {s} de {f} (o resto do arquivo é preservado) e vale na hora.', { s: '<code class="cmd">[alerts]</code>', f: `<code class="cmd">${esc(cfg.path)}</code>` })}</p>
        <div class="grid g2" style="gap:4px 16px">${RULE_FIELDS.map(([k, l]) => `<label class="field">${esc(t(l))}<input class="input" type="number" step="any" data-rule="${k}" value="${esc(v[k])}"></label>`).join('')}</div>
        ${cfg.warnings.length ? `<p class="muted">${esc(t('Avisos do arquivo: {w}', { w: cfg.warnings.map(tErr).join(' · ') }))}</p>` : ''}`,
      actions: [{ label: t('Cancelar'), value: null }, { label: t('Salvar'), cls: 'primary', onclick: () => {
        const out = {};
        $$('[data-rule]').forEach((i) => { out[i.dataset.rule] = Number(i.value); });
        closeDialog(out);
      } }],
    });
    if (!ok) return;
    try { await api('/api/config/alerts', { method: 'POST', body: ok }); toast(t('Limiares salvos.')); } catch (e) { toast(t('Não salvou: {m}', { m: e.message }), true); }
    loadAlerts();
  }
  async function loadChannels() {
    let c;
    try { c = await api('/api/config/notify'); } catch (e) { html('al-channels', errBox(e)); return; }
    const row = (name, val, extra = '') => `<dt>${esc(name)}</dt><dd>${val ? rawPill(val, 'ok') + extra : pill('não configurado')}</dd>`;
    html('al-channels', `<dl class="kv">
      ${row('ntfy', c.ntfy, c.ntfy_auth ? ` <span class="muted">${t('com token')}</span>` : '')}
      ${row(c.webhook_kind === 'discord' ? 'Webhook (Discord)' : 'Webhook (JSON)', c.webhook)}
      ${row('Telegram', c.telegram ? t('configurado') : '')}
      <dt>${t('Encaminhar alertas')}</dt><dd>${c.alerts ? pill('sim', 'ok') : pill('não')}</dd>
      <dt>${t('Encaminhar "agente pedindo atenção"')}</dt><dd>${c.agents ? pill('sim', 'ok') : pill('não')}</dd></dl>
      <p class="muted">${t('Segredos (tokens, URLs completas) ficam só no {f} (0600) e nunca voltam para a tela. O envio usa o {c}.', { f: '<code class="cmd">config.toml</code>', c: '<code class="cmd">curl</code>' })}</p>`);
    $('#al-ch-test').disabled = !(c.ntfy || c.webhook || c.telegram);
    S.channels = c;
  }
  async function editChannels() {
    const c = S.channels || {};
    const f = (id, label, hint, set, type = 'text') => `<label class="field">${esc(t(label))}<input class="input mono" id="${id}" type="${type}" autocomplete="off" spellcheck="false" placeholder="${esc(set ? t('•••• (configurado — deixe em branco para manter)') : hint)}"></label>`;
    const out = await dialog({
      title: t('Canais externos de notificação'),
      wide: true,
      body: `<p class="sub">${t('Envie alertas para o celular ou para um chat. Campos em branco mantêm o valor atual; marque “remover” para apagar um canal.')}</p>
        <div class="grid g2" style="gap:4px 16px">
          ${f('ch-ntfy', 'ntfy: URL do tópico', t('https://ntfy.sh/um-topico-dificil-de-adivinhar'), c.ntfy)}
          ${f('ch-ntfy-token', 'ntfy: token (opcional)', 'tk_…', c.ntfy_auth, 'password')}
          ${f('ch-webhook', 'Webhook (JSON genérico ou Discord)', 'https://discord.com/api/webhooks/…', c.webhook)}
          <div></div>
          ${f('ch-tg-token', 'Telegram: token do bot', '123456:ABC…', c.telegram, 'password')}
          ${f('ch-tg-chat', 'Telegram: chat id', '123456789', c.telegram)}
        </div>
        <div class="toolbar" style="margin:8px 0">
          <label class="chk"><input type="checkbox" id="ch-rm-ntfy"> ${t('remover ntfy')}</label>
          <label class="chk"><input type="checkbox" id="ch-rm-webhook"> ${t('remover webhook')}</label>
          <label class="chk"><input type="checkbox" id="ch-rm-tg"> ${t('remover Telegram')}</label>
        </div>
        <label class="chk"><input type="checkbox" id="ch-alerts" ${c.alerts !== false ? 'checked' : ''}> ${t('encaminhar alertas do sistema')}</label><br>
        <label class="chk"><input type="checkbox" id="ch-agents" ${c.agents ? 'checked' : ''}> ${t('encaminhar quando um agente pedir atenção')}</label>
        <p class="muted">${t('Tópicos do ntfy.sh são públicos para quem souber o nome: use um nome longo e aleatório ou um servidor com token.')}</p>`,
      actions: [{ label: t('Cancelar'), value: null }, { label: t('Salvar'), cls: 'primary', onclick: () => {
        const body = { alerts: $('#ch-alerts').checked, agents: $('#ch-agents').checked };
        const put = (k, id) => { const v = $('#' + id).value.trim(); if (v) body[k] = v; };
        put('ntfy_url', 'ch-ntfy'); put('ntfy_token', 'ch-ntfy-token'); put('webhook_url', 'ch-webhook');
        put('telegram_bot_token', 'ch-tg-token'); put('telegram_chat_id', 'ch-tg-chat');
        if ($('#ch-rm-ntfy').checked) { body.ntfy_url = ''; body.ntfy_token = ''; }
        if ($('#ch-rm-webhook').checked) body.webhook_url = '';
        if ($('#ch-rm-tg').checked) { body.telegram_bot_token = ''; body.telegram_chat_id = ''; }
        closeDialog(body);
      } }],
    });
    if (!out) return;
    try { await api('/api/config/notify', { method: 'POST', body: out }); toast(t('Canais salvos.')); } catch (e) { toast(t('Não salvou: {m}', { m: e.message }), true); }
    loadChannels();
  }
  async function testChannels() {
    try {
      const r = await api('/api/notify/test', { method: 'POST', body: {} });
      if (!r.results.length) { toast(t('Nenhum canal configurado.'), true); return; }
      const bad = r.results.filter((x) => !x.ok);
      toast(bad.length ? t('Falhou: {m}', { m: bad.map((x) => `${x.channel} (${tErr(x.error)})`).join(', ') }) : t('Teste enviado: {c}', { c: r.results.map((x) => x.channel).join(', ') }), !!bad.length);
    } catch (e) { toast(t('Falhou: {m}', { m: e.message }), true); }
  }
  page('alertas', 'Alertas', 'bell', {
    mount(el) {
      el.innerHTML = `${panel('Ativos agora', '<div id="al-active"></div>')}
        <div class="grid g-main">${card('Histórico', '<div id="al-hist"></div>')}<div class="grid" style="align-content:start">${card('Regras', '<div class="table-wrap" id="al-rules"></div>', `<span class="sp"></span><button class="btn sm" id="al-edit">${t('Editar limiares')}</button>`)}${card('Notificação no desktop', '<div class="toolbar" id="al-notify"></div>')}${card('Canais externos', `<div id="al-channels">${empty('Carregando…')}</div>`, `<span class="sp"></span><button class="btn sm" id="al-ch-test">${t('Enviar teste')}</button><button class="btn sm" id="al-ch-edit">${t('Configurar')}</button>`)}</div></div>`;
      $('#al-edit').addEventListener('click', editRules);
      $('#al-ch-edit').addEventListener('click', editChannels);
      $('#al-ch-test').addEventListener('click', testChannels);
      loadChannels();
      loadAlerts();
    },
    unmount() { clearTimeout(alertsTimer); },
  });

  // ------------------------------------------------------------ cabeçalho e rotas
  function renderNav() {
    $('#nav').innerHTML = PAGES.map((p) => `<a class="nav-item" href="#/${p.id}" data-id="${p.id}">${ICONS[p.icon]}<span>${esc(t(p.title))}</span>${p.id === 'alertas' ? '<span class="badge" id="nav-badge" hidden></span>' : ''}</a>`).join('');
    if (S.page) $$('.nav-item').forEach((a) => (a.dataset.id === S.page.id ? a.setAttribute('aria-current', 'page') : a.removeAttribute('aria-current')));
  }
  function renderHeader(s) {
    $('#page-sub').textContent = t('{os} · kernel {k} · ligado há {d}', { os: s.host.os, k: s.host.kernel, d: dur(s.host.uptime_s) });
    $('#brand-host').textContent = s.host.hostname;
    S.user = s.host.user;
    $('#chip-self').textContent = `Pulse ${pct(s.pulse.cpu)} CPU · ${bytes(s.pulse.rss)}`;
    const n = s.alerts.length;
    const crit = s.alerts.some((a) => a.severity === 'critico');
    const ch = $('#chip-alerts');
    ch.textContent = n === 1 ? t('1 alerta') : t('{n} alertas', { n });
    ch.className = 'chip' + (n ? (crit ? ' crit' : ' warn') : '');
    const b = $('#nav-badge');
    if (b) { b.hidden = !n; b.textContent = n; b.className = 'badge' + (crit ? ' crit' : ''); }
    document.title = n ? `(${n}) Pulse` : 'Pulse';
  }
  // Seletor de máquina (só aparece com [remote.*] configurado).
  async function loadRemotes() {
    try { S.remotes = await api('/api/remotes', { local: true }); } catch { S.remotes = []; }
    if (S.remote && !S.remotes.some((r) => r.name === S.remote)) { S.remote = ''; session.del('pulse.remote'); }
    const sel = $('#host-sel');
    sel.hidden = !S.remotes.length;
    if (!S.remotes.length) return;
    const st = { ok: '', conectando: ' ' + t('(conectando)'), erro: ' ' + t('(erro)') };
    sel.innerHTML = `<option value="">${esc(t('Esta máquina'))}</option>` + S.remotes.map((r) => `<option value="${esc(r.name)}"${r.name === S.remote ? ' selected' : ''}${r.state === 'ok' || r.name === S.remote ? '' : ' disabled'}>${esc(r.name)}${r.hostname && r.hostname !== r.name ? ' · ' + esc(r.hostname) : ''}${esc(st[r.state] ?? '')}</option>`).join('');
  }
  async function switchMachine(name) {
    S.remote = name;
    if (name) session.set('pulse.remote', name); else session.del('pulse.remote');
    document.documentElement.classList.toggle('remote', !!name);
    S.snap = null;
    S.lastSeq = 0;
    S.agents = null;
    for (const k in S.hist) S.hist[k] = [];
    if (S.es) S.es.close();
    await loadInitial();
    route();
    startStream();
    toast(name ? t('Vendo {n} (somente leitura)', { n: name }) : t('Vendo esta máquina'));
  }
  async function loadInitial() {
    try {
      const hist = await api('/api/history');
      for (const k in S.hist) S.hist[k] = (hist[k] || []).map((v) => (v == null ? null : v));
      // a GPU não está no anel do servidor: começa vazia, alinhada ao tempo
      S.hist.gpu = S.hist.ts.map(() => null);
    } catch { /* começa vazio */ }
    try { const snap = await api('/api/snapshot'); if (snap && snap.cpu) { S.snap = snap; S.lastSeq = snap.seq; renderHeader(snap); } } catch { /* idem */ }
  }
  $('#host-sel').addEventListener('change', (e) => switchMachine(e.target.value));
  function route() {
    const id = (location.hash.match(/^#\/([\w-]+)/) || [])[1] || 'visao-geral';
    const p = PAGES.find((x) => x.id === id) || PAGES[0];
    if (S.page && S.page.unmount) S.page.unmount();
    S.page = p;
    $$('.nav-item').forEach((a) => (a.dataset.id === p.id ? a.setAttribute('aria-current', 'page') : a.removeAttribute('aria-current')));
    $('#page-title').textContent = t(p.title);
    const el = $('#page');
    el.innerHTML = '';
    p.mount(el);
    if (S.snap && p.update) p.update(S.snap);
    el.focus({ preventScroll: true });
    window.scrollTo(0, 0);
  }
  window.addEventListener('hashchange', route);

  // Atalhos: g + letra navega, / busca, t tema, ? ajuda.
  const GO = { v: 'visao-geral', c: 'cpu', m: 'memoria', d: 'discos', p: 'processos', s: 'servicos', r: 'rede', l: 'logs', a: 'agentes', x: 'alertas' };
  let gPending = 0;
  function shortcutsHelp() {
    const row = (k, d) => `<dt><code class="cmd">${esc(k)}</code></dt><dd style="text-align:left;font-weight:500">${esc(t(d))}</dd>`;
    dialog({
      title: t('Atalhos de teclado'),
      body: `<dl class="kv" style="grid-template-columns:auto 1fr">${Object.entries(GO).map(([k, id]) => row('g ' + k, PAGES.find((x) => x.id === id).title)).join('')}
        ${row('/', 'buscar/filtrar na tela atual')}${row('t', 'alternar tema (auto → claro → escuro)')}${row('?', 'esta ajuda')}${row('Esc', 'fechar diálogos')}</dl>`,
      actions: [{ label: t('Fechar'), cls: 'primary', value: true }],
    });
  }
  document.addEventListener('keydown', (e) => {
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    const tg = e.target;
    if (tg && (tg.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(tg.tagName))) return;
    if (!$('#overlay').hidden) return;
    const k = e.key;
    if (gPending && Date.now() - gPending < 1500 && GO[k]) {
      gPending = 0;
      e.preventDefault();
      location.hash = '#/' + GO[k];
      return;
    }
    gPending = 0;
    if (k === 'g') { gPending = Date.now(); return; }
    if (k === '/') {
      const f = $('#page input[type="search"], #page input.input:not([type="checkbox"])');
      if (f) { e.preventDefault(); f.focus(); f.select && f.select(); }
      return;
    }
    if (k === '?') { e.preventDefault(); shortcutsHelp(); return; }
    if (k === 't') {
      const order = ['auto', 'light', 'dark'];
      const cur = store.get('pulse.theme') || 'auto';
      const next = order[(order.indexOf(cur) + 1) % order.length];
      store.set('pulse.theme', next);
      applyTheme(next);
      toast(t('Tema: {t}', { t: t({ auto: 'automático', light: 'claro', dark: 'escuro' }[next]) }));
    }
  });
  let resizeT = 0;
  window.addEventListener('resize', () => { clearTimeout(resizeT); resizeT = setTimeout(() => { if (S.snap && S.page && S.page.update) S.page.update(S.snap); }, 150); });

  let raf = 0;
  function onSnapshot(s) {
    S.snap = s;
    pushPoint(s);
    if (raf) return;
    raf = requestAnimationFrame(() => {
      raf = 0;
      if (!S.snap) return; // trocou de máquina neste quadro
      renderHeader(S.snap);
      if (S.page && S.page.update) S.page.update(S.snap);
    });
  }

  function startStream() {
    if (S.es) S.es.close();
    const es = new EventSource(remotePath('/api/stream'));
    S.es = es;
    es.addEventListener('snapshot', (e) => { try { onSnapshot(JSON.parse(e.data)); } catch { /* ignora */ } });
    es.addEventListener('hook', () => { if (S.page && (S.page.id === 'agentes' || S.page.id === 'visao-geral')) pollAgents(); });
    es.onerror = async () => {
      // EventSource reconecta sozinho; se a sessão expirou, pede login.
      try { const h = await (await fetch('/api/health', { credentials: 'same-origin' })).json(); if (!h.authenticated) { es.close(); showLogin(); } } catch { /* daemon fora do ar */ }
    };
  }

  // ------------------------------------------------------------ tema
  function applyTheme(t) {
    if (t === 'light' || t === 'dark') document.documentElement.dataset.theme = t; else delete document.documentElement.dataset.theme;
    $$('[data-theme-opt]').forEach((b) => b.setAttribute('aria-checked', String(b.dataset.themeOpt === (t || 'auto'))));
  }
  $$('[data-theme-opt]').forEach((b) => b.addEventListener('click', () => { store.set('pulse.theme', b.dataset.themeOpt); applyTheme(b.dataset.themeOpt); }));
  applyTheme(store.get('pulse.theme') || 'auto');

  // ------------------------------------------------------------ idioma
  /** Textos fixos do index.html: data-i18n (textContent), data-i18n-aria, data-i18n-placeholder. */
  function applyStatic() {
    $$('[data-i18n]').forEach((el) => { el.textContent = t(el.dataset.i18n); });
    $$('[data-i18n-aria]').forEach((el) => el.setAttribute('aria-label', t(el.dataset.i18nAria)));
    $$('[data-i18n-placeholder]').forEach((el) => el.setAttribute('placeholder', t(el.dataset.i18nPlaceholder)));
    $$('[data-lang-opt]').forEach((b) => b.setAttribute('aria-checked', String(b.dataset.langOpt === LANG)));
    document.documentElement.lang = LANG === 'en' ? 'en' : 'pt-BR';
  }
  function setLang(l) {
    if (l !== 'pt' && l !== 'en') return;
    LANG = l;
    store.set('pulse.lang', l);
    applyStatic();
    renderNav();
    if (S.snap) renderHeader(S.snap);
    if (S.remotes.length) loadRemotes();
    if (S.page) route();
  }
  $$('[data-lang-opt]').forEach((b) => b.addEventListener('click', () => setLang(b.dataset.langOpt)));
  // Refração SVG só no Chromium; os demais ficam com o blur do CSS.
  try {
    const brands = (navigator.userAgentData && navigator.userAgentData.brands) || [];
    if (brands.some((b) => /Chromium/i.test(b.brand))) document.documentElement.classList.add('refract');
  } catch { /* fallback de blur */ }

  // ------------------------------------------------------------ início
  if ('serviceWorker' in navigator && window.isSecureContext) {
    navigator.serviceWorker.register('/sw.js').catch(() => { /* instalação como app é opcional */ });
  }

  // ------------------------------------------------------------ dicionário inglês
  // Chave = frase em pt-BR exatamente como aparece no código; ausente → mostra o pt-BR.
  const EN = {
    // index.html e estrutura
    'Navegação': 'Navigation', 'Tema': 'Theme', 'Idioma': 'Language', 'Auto': 'Auto', 'Claro': 'Light', 'Escuro': 'Dark',
    'Conectando…': 'Connecting…', 'Máquina': 'Machine', '0 alertas': '0 alerts', '1 alerta': '1 alert', '{n} alertas': '{n} alerts',
    '{os} · kernel {k} · ligado há {d}': '{os} · kernel {k} · up {d}',
    'Esta máquina': 'This machine', '(conectando)': '(connecting)', '(erro)': '(error)',
    'Vendo {n} (somente leitura)': 'Viewing {n} (read-only)', 'Vendo esta máquina': 'Viewing this machine',
    'Daemon indisponível.': 'Daemon unavailable.', 'Token da URL inválido.': 'Invalid token in the URL.',
    // páginas
    'Visão geral': 'Overview', 'Memória': 'Memory', 'Discos': 'Disks', 'Processos': 'Processes', 'Serviços': 'Services',
    'Rede': 'Network', 'Logs': 'Logs', 'Agentes': 'Agents', 'Alertas': 'Alerts',
    // tempo e genéricos
    'agora': 'now', 'há {v}': '{v} ago', 'em {d}': 'in {d}', 'gráfico': 'chart', 'hoje': 'today',
    'Carregando…': 'Loading…', 'Sem dados.': 'No data.', 'Erro: {m}': 'Error: {m}', 'Falhou: {m}': 'Failed: {m}',
    'Não salvou: {m}': 'Not saved: {m}', 'Feito.': 'Done.', 'Cancelar': 'Cancel', 'Fechar': 'Close', 'Salvar': 'Save',
    'Aplicar': 'Apply', 'Atualizar': 'Refresh', 'Configurar': 'Configure', 'Remover': 'Remove', 'Verificar': 'Check',
    'sim': 'yes', 'não': 'no', 'nenhum': 'none', 'indisponível': 'unavailable', 'Crítico': 'Critical', 'Aviso': 'Warning',
    '{u} de {t}': '{u} of {t}', 'de {t}': 'of {t}', 'de {t} · {p}': 'of {t} · {p}', '{a} de {b}': '{a} of {b}',
    // acesso
    'Acesso ao Pulse': 'Pulse access', 'cole o token aqui': 'paste the token here', 'Entrar': 'Sign in',
    'Obtenha a URL de acesso com {a} ou leia {b}.': 'Get the access URL with {a} or read {b}.',
    'Sessão iniciada.': 'Signed in.', 'Token inválido.': 'Invalid token.', 'token inválido': 'invalid token',
    'Informe o token de acesso para ver o painel.': 'Enter the access token to view the dashboard.',
    'Para executar ações, informe o token de acesso.': 'Enter the access token to run actions.',
    '{m} é somente leitura aqui: abra o Pulse dessa máquina para executar ações': '{m} is read-only here: open that machine\'s Pulse to run actions',
    // visão geral
    'CPU e memória': 'CPU and memory', 'comparar com o período anterior': 'compare with the previous period',
    'Maiores consumidores': 'Top consumers', 'Agentes agora': 'Agents now', 'ver agentes': 'view agents', 'Armazenamento': 'Storage',
    'hora anterior': 'previous hour', 'ontem': 'yesterday', 'semana anterior': 'previous week',
    'Histórico longo: {m}': 'Long history: {m}',
    'tracejado = {p} · CPU média {a} agora × {b} antes': 'dashed = {p} · average CPU {a} now vs {b} before',
    '{n} amostras de 1 s': '{n} samples of 1 s', '{n} médias de {b} (histórico longo)': '{n} averages of {b} (long history)',
    'carga {l} · {n} núcleos': 'load {l} · {n} cores', 'swap desativada': 'swap disabled', 'Disco': 'Disk',
    '{a} livres em {m}': '{a} free on {m}', 'Temperatura': 'Temperature', 'sem sensores disponíveis': 'no sensors available',
    'Nenhum alerta ativo.': 'No active alerts.', 'Nenhum sistema de arquivos.': 'No file systems.',
    'Processo': 'Process',
    // estados de agentes
    'Executando': 'Running', 'Pedindo atenção': 'Needs attention', 'Aguardando': 'Waiting', 'Concluído': 'Done',
    'executando (inferido)': 'running (inferred)', 'ativa': 'active', 'inativa': 'inactive', 'ativo': 'active', 'pausado': 'paused',
    'Detecção de agentes desativada (--no-agents).': 'Agent detection disabled (--no-agents).',
    'Nenhuma sessão recente.': 'No recent sessions.', 'Nenhum processo de agente em execução.': 'No agent processes running.',
    '1 processo de agente em execução: {a}.': '1 agent process running: {a}.',
    '{n} processos de agente em execução: {a}.': '{n} agent processes running: {a}.',
    // CPU
    'Histórico': 'History', 'Detalhes': 'Details', 'Núcleos': 'Cores', 'Temperaturas': 'Temperatures',
    'Uso total': 'Total usage', 'Usuário': 'User', 'Sistema': 'System', 'Carga 1/5/15 min': 'Load 1/5/15 min',
    'Executando / bloqueados': 'Running / blocked', 'Trocas de contexto': 'Context switches', 'Pressão (PSI)': 'Pressure (PSI)',
    'núcleo {n}': 'core {n}', 'Nenhum sensor de temperatura disponível.': 'No temperature sensors available.',
    'GTT (memória do sistema usada pela GPU)': 'GTT (system memory used by the GPU)',
    'carregando': 'charging', 'descarregando': 'discharging', 'cheia': 'full', 'sem carregar': 'not charging',
    'Bateria {n} · {s}': 'Battery {n} · {s}', '{d} para carregar': '{d} to full', '{d} restantes': '{d} left',
    'saúde {p}': 'health {p}', 'Pacote da CPU (RAPL)': 'CPU package (RAPL)', 'na tomada': 'on AC power', 'na bateria': 'on battery',
    'Energia': 'Power',
    // memória
    'Composição': 'Breakdown', 'Maiores consumidores de memória': 'Top memory consumers', 'Quem está na swap': 'Who is in swap',
    'Em uso': 'In use', 'Disponível': 'Available', 'livre + cache recuperável': 'free + reclaimable cache',
    'Aplicativos': 'Applications', 'Livre': 'Free', 'Memória compartilhada (shmem)': 'Shared memory (shmem)',
    'Páginas sujas': 'Dirty pages', 'Em writeback': 'In writeback', 'Swap em cache': 'Swap cached',
    '{a} / {b} pág/s': '{a} / {b} pages/s', 'Faltas de página maiores': 'Major page faults',
    'Mortes por OOM (desde o boot)': 'OOM kills (since boot)', 'Pressão (PSI) 10 s': 'Pressure (PSI) 10 s',
    'Na swap': 'In swap', 'Na RAM': 'In RAM',
    'Muita memória na swap com "swap in" alto deixa o sistema lento: fechar ou reiniciar esses processos libera a swap.': 'Lots of memory in swap with high "swap in" slows the system down: closing or restarting these processes frees swap.',
    'Nenhum processo legível está usando a swap.': 'No readable process is using swap.', 'Swap desativada.': 'Swap disabled.',
    // discos
    'Varrendo {p} em segundo plano… {n} entradas em {s} s. Você pode sair desta tela; o resultado fica guardado por 5 min.': 'Scanning {p} in the background… {n} entries in {s} s. You can leave this screen; the result is kept for 5 min.',
    'varredura parcial (limite de tempo/arquivos)': 'partial scan (time/file limit)', '{n} sem permissão': '{n} without permission',
    'calculado {a}': 'calculated {a}', 'total {b}': 'total {b}', 'Nome': 'Name', 'Proporção': 'Share', 'Tamanho': 'Size',
    'Diretório vazio.': 'Empty directory.', 'sem permissão': 'permission denied', 'Ler com senha': 'Read with password',
    'aprovado': 'passed', 'FALHOU': 'FAILED', 'Aguardando a senha…': 'Waiting for the password…',
    'Consultando o smartctl (≈ 1 s por disco)…': 'Querying smartctl (≈ 1 s per disk)…', 'Para habilitar:': 'To enable:',
    'Ler SMART exige root ou o grupo {g}; o Pulse só pede a senha quando você clica em “Ler com senha”.': 'Reading SMART requires root or the {g} group; Pulse only asks for the password when you click “Read with password”.',
    'Modelo': 'Model', 'Saúde': 'Health', 'Horas ligado': 'Power-on hours', 'Setores realocados / pendentes': 'Reallocated / pending sectors',
    'NVMe: desgaste / erros': 'NVMe: wear / errors', 'Sistemas de arquivos': 'File systems', 'Leitura': 'Read', 'Escrita': 'Write',
    'Pressão de E/S': 'I/O pressure', 'Dispositivos': 'Devices', 'Saúde dos discos (SMART)': 'Disk health (SMART)',
    'Leitura sob demanda via {c}.': 'On-demand reading via {c}.', 'O que ocupa espaço?': 'What is using space?',
    'Pasta pessoal': 'Home folder', 'Candidatos à limpeza': 'Cleanup candidates',
    'O Pulse <b>nunca executa</b> estes comandos — só sugere. Revise antes de rodar no terminal.': 'Pulse <b>never runs</b> these commands — it only suggests them. Review before running them in a terminal.',
    'Calculando tamanhos…': 'Calculating sizes…', 'Item': 'Item', 'Caminho': 'Path', 'Comando sugerido': 'Suggested command',
    'Nada a sugerir.': 'Nothing to suggest.', 'Montagem': 'Mount', 'Dispositivo': 'Device', 'Tipo': 'Type', 'Uso': 'Usage',
    'Usado / total': 'Used / total', 'Leitura agora': 'Read now', 'Escrita agora': 'Write now', 'IOPS (l/e)': 'IOPS (r/w)',
    'Ocupação': 'Busy', 'Nenhum dispositivo de bloco.': 'No block devices.',
    // processos
    'rodando': 'running', 'dormindo': 'sleeping', 'E/S': 'I/O', 'ocioso': 'idle', 'zumbi': 'zombie', 'parado': 'stopped',
    'rastreado': 'traced', 'morto': 'dead', 'Total': 'Total', 'Rodando': 'Running', 'Dormindo': 'Sleeping', 'Zumbis': 'Zombies',
    'Parados': 'Stopped', 'sem permissão para /proc/PID/io': 'no permission for /proc/PID/io', 'Continuar': 'Resume',
    'Pausar': 'Pause', 'Prioridade': 'Priority', 'Encerrar': 'Terminate', '{n} processos na árvore': '{n} processes in the tree',
    '(limite de 400)': '(400 limit)', 'CPU (árvore)': 'CPU (tree)',
    'Soma do RSS da subárvore; memória compartilhada entra mais de uma vez': 'Sum of the subtree RSS; shared memory is counted more than once',
    'RSS somado': 'Summed RSS', 'Ações': 'Actions',
    'RSS somado conta memória compartilhada (bibliotecas, abas do mesmo navegador) mais de uma vez; use como ordem de grandeza.': 'Summed RSS counts shared memory (libraries, tabs of the same browser) more than once; use it as an order of magnitude.',
    'mostrando 300 de {n}': 'showing 300 of {n}', '{n} processos': '{n} processes', 'Estado': 'State', 'Disco L/E': 'Disk R/W',
    'Comando': 'Command', 'A ação é registrada no log de auditoria.': 'The action is recorded in the audit log.',
    'Encerrar processo?': 'Terminate process?',
    'Enviar SIGTERM para {p}? Se não encerrar em 5 s, o Pulse envia SIGKILL.': 'Send SIGTERM to {p}? If it does not exit within 5 s, Pulse sends SIGKILL.',
    'Pausar processo?': 'Pause process?', 'Enviar SIGSTOP para {p}? Ele fica congelado até ser continuado.': 'Send SIGSTOP to {p}? It stays frozen until resumed.',
    'Continuar processo?': 'Resume process?', 'Enviar SIGCONT para {p}?': 'Send SIGCONT to {p}?', 'Ajustar prioridade': 'Adjust priority',
    'Novo valor de <i>nice</i> para {p} (−20 = maior prioridade, 19 = menor). Diminuir o nice exige privilégios.': 'New <i>nice</i> value for {p} (−20 = highest priority, 19 = lowest). Lowering nice requires privileges.',
    'Filtrar por nome, comando, PID ou usuário': 'Filter by name, command, PID or user', 'só meus': 'mine only', 'Lista': 'List', 'Árvore': 'Tree',
    // serviços
    'Unidade': 'Unit', 'Descrição': 'Description', 'Habilitado': 'Enabled', 'Reiniciar': 'Restart', 'Parar': 'Stop', 'Iniciar': 'Start',
    'falhou': 'failed', 'iniciando': 'starting', 'inativo': 'inactive', 'reiniciando': 'restarting',
    'Nenhum serviço corresponde ao filtro.': 'No services match the filter.', 'Ativa': 'Activates', 'Próxima execução': 'Next run',
    'Última execução': 'Last run', 'Nenhum timer neste escopo.': 'No timers in this scope.', 'a cada hora': 'hourly', 'diário': 'daily',
    'semanal': 'weekly', 'mensal': 'monthly', 'Agenda': 'Schedule', 'Origem': 'Source', 'Nenhuma entrada de cron.': 'No cron entries.',
    'Variáveis de ambiente do crontab não são exibidas e valores que parecem senhas/tokens são mascarados.': 'Crontab environment variables are not shown and values that look like passwords/tokens are masked.',
    'Logs: {m}': 'Logs: {m}', 'Logs de {n}': 'Logs of {n}',
    'Últimas 300 linhas (stdout + stderr, horário UTC).': 'Last 300 lines (stdout + stderr, UTC time).',
    '{e} indisponível: {m}': '{e} unavailable: {m}', '{e} {v} · {a} de {n} em execução': '{e} {v} · {a} of {n} running',
    'Contêineres (Docker/Podman)': 'Containers (Docker/Podman)', 'disco não encontrado': 'disk not found', 'já instalado': 'already installed',
    'nem o Docker nem o Podman estão instalados': 'neither Docker nor Podman is installed', 'sem permissão para falar com o Podman': 'no permission to talk to Podman',
    'o config.toml já tem um notify de outro programa; nada foi alterado (o Codex aceita só um notify)': 'config.toml already has a notify from another program; nothing was changed (Codex accepts only one notify)',
    'sem saída': 'no output', 'Docker indisponível: {m}': 'Docker unavailable: {m}', 'Docker {v} · {a} de {n} em execução': 'Docker {v} · {a} of {n} running',
    'Contêiner': 'Container', 'Imagem': 'Image', 'Portas': 'Ports', 'Nenhum contêiner.': 'No containers.', 'Todos': 'All', 'Ativos': 'Active',
    'Falhas': 'Failed', 'Inativos': 'Inactive', 'Buscar serviço': 'Search services', 'Timers do systemd': 'systemd timers',
    'Contêineres (Docker)': 'Containers (Docker)', 'Medir CPU/memória': 'Measure CPU/memory', 'escopo: usuário': 'scope: user',
    'escopo: sistema': 'scope: system',
    'Ações no escopo do <b>sistema</b> abrem a janela de senha do polkit (helper do Pulse via {c}, sem sudo); a autorização vale por alguns minutos. Sem agente polkit na sessão, a ação falha com aviso.': 'Actions in the <b>system</b> scope open the polkit password prompt (Pulse helper via {c}, no sudo); the authorization lasts a few minutes. Without a polkit agent in the session, the action fails with a warning.',
    'Serviços do seu usuário ({c}).': 'Your user services ({c}).', 'Iniciar serviço?': 'Start service?', 'Parar serviço?': 'Stop service?',
    'Reiniciar serviço?': 'Restart service?', '{v} {u} (escopo {s})?': '{v} {u} ({s} scope)?', 'usuário': 'user', 'sistema': 'system',
    'Medindo (docker stats leva alguns segundos)…': 'Measuring (docker stats takes a few seconds)…',
    'Iniciar contêiner?': 'Start container?', 'Parar contêiner?': 'Stop container?', 'Reiniciar contêiner?': 'Restart container?',
    // rede
    '{n} operações de disco · latência média {a} ms · máxima {m} ms': '{n} disk operations · average latency {a} ms · max {m} ms',
    'nenhuma operação de disco no intervalo': 'no disk operations in the interval', 'Últimos {s} s · {d}': 'Last {s} s · {d}',
    '↓ média': '↓ average', '↑ média': '↑ average', 'Recebido': 'Received', 'Enviado': 'Sent',
    'Nenhum tráfego TCP/UDP no intervalo.': 'No TCP/UDP traffic in the interval.', 'Medindo…': 'Measuring…',
    'Aguardando a senha e medindo por {s} s…': 'Waiting for the password and measuring for {s} s…', 'Medir': 'Measure',
    'Tráfego': 'Traffic', 'Tráfego por processo (eBPF)': 'Traffic per process (eBPF)',
    'Medição sob demanda com bpftrace: bytes enviados/recebidos por processo e latência dos discos no intervalo. Pede sua senha (polkit) e precisa do pacote {p}.': 'On-demand measurement with bpftrace: bytes sent/received per process and disk latency over the interval. Asks for your password (polkit) and needs the {p} package.',
    'Duração': 'Duration', 'Portas em escuta': 'Listening ports', 'processos de outros usuários aparecem sem dono': 'other users\' processes appear without an owner',
    'todas as interfaces físicas': 'all physical interfaces', 'Conexões TCP': 'TCP connections',
    'estabelecidas · escutando {n}': 'established · listening {n}', 'Velocidade': 'Speed', '↓ agora': '↓ now', '↑ agora': '↑ now',
    'Erros/descartes': 'Errors/drops', 'virtual': 'virtual', 'desconhecido': 'unknown', 'Endereço': 'Address', 'Porta': 'Port',
    'Alcance': 'Reach', 'só local': 'local only', 'acessível na rede': 'reachable on the network', 'Nenhuma porta em escuta.': 'No listening ports.',
    // logs
    'emergência': 'emergency', 'alerta': 'alert', 'crítico': 'critical', 'erro': 'error', 'aviso': 'warning', 'notícia': 'notice',
    'erro+': 'error+', 'Nenhuma linha.': 'No lines.', '{n} linhas': '{n} lines', 'unidade (ex.: ssh.service)': 'unit (e.g. ssh.service)',
    'todas as prioridades': 'all priorities', 'até {i} · {n}': 'up to {i} · {n}', 'Linhas': 'Lines', 'Buscar texto': 'Search text', 'seguir': 'follow',
    // agentes
    'Processos de agentes': 'Agent processes', 'Servidores MCP': 'MCP servers', 'nenhum detectado': 'none detected',
    'Sessões ativas / recentes': 'Active / recent sessions', 'ativas: últimos 5 min · recentes: 24 h': 'active: last 5 min · recent: 24 h',
    'Tokens hoje': 'Tokens today', 'custo estimado {c}': 'estimated cost {c}', '(parcial: há modelos sem preço)': '(partial: some models have no price)',
    'cadastre preços em Preferências para ver o custo': 'add prices in Preferences to see the cost', 'sessão {id}': 'session {id}',
    '{n} subagentes rodando': '{n} subagents running', '1 subagente rodando': '1 subagent running', 'última: {m}': 'last: {m}',
    '{n} falhas de ferramenta': '{n} tool failures', '1 falha de ferramenta': '1 tool failure',
    'Inferido pela transcrição em atividade (o notify do Codex só avisa no fim do turno).': 'Inferred from transcript activity (Codex notify only fires at the end of a turn).',
    'Nenhum evento de hook recebido ainda. Use “Conectar hooks” para o Claude Code enviar estados ao vivo.': 'No hook events received yet. Use “Connect hooks” so Claude Code sends live states.',
    '1 filho': '1 child', '{n} filhos': '{n} children', 'Agente': 'Agent', 'Projeto': 'Project',
    'pedidos do usuário / chamadas de ferramentas': 'user prompts / tool calls', 'Pedidos / ferr.': 'Prompts / tools',
    'Ativo / esperando você': 'Active / waiting for you', 'Tokens in/out': 'Tokens in/out', 'Custo': 'Cost', 'Atividade': 'Activity',
    '{n} msgs de subagentes': '{n} subagent msgs', '{n} msgs': '{n} msgs',
    'Nenhuma sessão recente em ~/.claude/projects, ~/.codex/sessions ou no OpenClaw.': 'No recent sessions in ~/.claude/projects, ~/.codex/sessions or OpenClaw.',
    'Sem eventos.': 'No events.', 'Pedido': 'Prompt', 'Resposta': 'Reply', 'Linha do tempo': 'Timeline', 'Ferramenta': 'Tool',
    'sem eventos': 'no events', 'Tempo ativo': 'Active time', 'Esperando você': 'Waiting for you',
    'Tokens: entrada / saída': 'Tokens: input / output', 'Cache: leitura / escrita': 'Cache: read / write', 'Custo estimado': 'Estimated cost',
    'sem preço para {m}': 'no price for {m}', 'Chamadas': 'Calls', 'Média': 'Average', 'Máx.': 'Max',
    'Transcrição grande: só o trecho final foi lido.': 'Large transcript: only the final part was read.',
    'Modo detalhado ligado: a linha do tempo mostra comandos e caminhos das ferramentas (mascarados).': 'Detailed mode on: the timeline shows tool commands and paths (masked).',
    'O texto dos pedidos e respostas nunca é exibido. "Esperando você" = do fim de uma resposta até o próximo pedido.': 'The text of prompts and replies is never shown. "Waiting for you" = from the end of a reply to the next prompt.',
    'Ainda sem dados (o índice dos últimos 30 dias é montado em segundo plano a cada 5 min).': 'No data yet (the 30-day index is built in the background every 5 min).',
    'Entrada': 'Input', 'Saída': 'Output', 'Cache (leitura / escrita)': 'Cache (read / write)', 'sem preço': 'no price',
    'Total com preço cadastrado em 30 dias: {c}. Custos são estimativas a partir da tabela de preços que você cadastra (US$ por milhão de tokens); assinaturas com franquia não cobram por token.': 'Total with a configured price over 30 days: {c}. Costs are estimates based on the price table you set up (US$ per million tokens); subscription plans do not charge per token.',
    'modelo': 'model', 'Preferências dos agentes': 'Agent preferences',
    'Avisar no desktop quando um agente pedir atenção (permissão, pergunta) — no máximo 1 aviso por minuto por sessão': 'Show a desktop notification when an agent needs attention (permission, question) — at most 1 per minute per session',
    'Modo detalhado: mostrar comandos e caminhos das ferramentas na linha do tempo e nos eventos ao vivo (valores que parecem senhas/tokens são mascarados; o texto das conversas nunca aparece)': 'Detailed mode: show tool commands and paths in the timeline and live events (values that look like passwords/tokens are masked; conversation text never appears)',
    'Preços (US$ por milhão de tokens)': 'Prices (US$ per million tokens)',
    'O Pulse não traz preços prontos: confira os valores atuais no site do provedor. Um modelo casa pelo nome exato ou pelo prefixo mais longo (ex.: {c} vale para todas as versões).': 'Pulse ships no prices: check current values on the provider\'s site. A model matches by exact name or by the longest prefix (e.g. {c} applies to every version).',
    'Cache: leitura': 'Cache: read', 'Cache: escrita': 'Cache: write', 'Adicionar modelo': 'Add model', 'Preferências salvas.': 'Preferences saved.',
    'Conectar hooks': 'Connect hooks',
    'Os hooks fazem o Claude Code avisar o Pulse quando está <b>executando</b>, <b>pedindo atenção</b> (inclusive pedidos de permissão), <b>aguardando</b> ou <b>concluído</b>, além de subagentes e falhas de ferramentas.': 'Hooks make Claude Code tell Pulse when it is <b>running</b>, <b>needs attention</b> (including permission requests), is <b>waiting</b> or <b>done</b>, plus subagents and tool failures.',
    'Arquivo': 'File', 'Script instalado': 'Script installed', 'não — rode install.sh': 'no — run install.sh', 'Eventos conectados': 'Connected events',
    'Instalar adiciona os eventos que faltam ao {a}, preservando o que já existe e criando um backup {b}:': 'Installing adds the missing events to {a}, keeping what is already there and creating a {b} backup:',
    'conectado': 'connected', 'não conectado': 'not connected', 'Conectar Codex': 'Connect Codex',
    '“Conectar Codex” grava o {n} no início de {f} (com backup). Se já existir outro {n} lá, nada é alterado e o motivo aparece num aviso. Alternativa: adicione você mesmo, antes de qualquer {t}:': '“Connect Codex” writes {n} at the top of {f} (with a backup). If another {n} is already there, nothing changes and the reason is shown. Alternatively, add it yourself before any {t}:',
    '[tabela]': '[table]', 'estado': 'state', 'OpenClaw: chame {c} a partir das notificações dele.': 'OpenClaw: call {c} from its notifications.',
    'Já conectado': 'Already connected', 'Instalar no Claude Code': 'Install in Claude Code', 'Codex conectado · backup em {b}': 'Codex connected · backup at {b}',
    'Codex conectado.': 'Codex connected.', 'Hooks adicionados: {h}': 'Hooks added: {h}', 'backup em {b}': 'backup at {b}',
    'Os hooks já estavam conectados.': 'The hooks were already connected.',
    'Processos, sessões e eventos dos agentes de IA rodando nesta máquina.': 'Processes, sessions and events of the AI agents running on this machine.',
    'Preferências': 'Preferences', 'Ao vivo (hooks)': 'Live (hooks)', 'Sessões': 'Sessions', 'Uso por dia (30 dias)': 'Daily usage (30 days)',
    'Eventos recentes (hooks)': 'Recent events (hooks)',
    // alertas
    'desde {h} ({a})': 'since {h} ({a})', 'Tudo certo — nenhum alerta ativo.': 'All good — no active alerts.', 'Regra': 'Rule',
    'Condição': 'Condition', 'Severidade': 'Severity', 'Quando': 'When', 'Evento': 'Event', 'Alerta': 'Alert', 'Detalhe': 'Detail',
    'resolvido': 'resolved', 'agravou': 'escalated', 'disparou': 'fired', 'Nenhum alerta registrado ainda.': 'No alerts recorded yet.',
    'ativada': 'enabled', 'desativada': 'disabled', 'Alertas novos vão para o desktop via {c}.': 'New alerts go to the desktop via {c}.',
    'Inicie o Pulse com {c} para receber notificações no desktop.': 'Start Pulse with {c} to get desktop notifications.',
    'CPU alta (%)': 'High CPU (%)', '…por N amostras': '…for N samples', 'Memória: aviso (%)': 'Memory: warning (%)',
    'Memória: crítico (%)': 'Memory: critical (%)', 'Swap: aviso (%)': 'Swap: warning (%)', 'Disco: aviso (%)': 'Disk: warning (%)',
    'Disco: crítico (%)': 'Disk: critical (%)', 'Temperatura (°C)': 'Temperature (°C)', 'Processo preso (% CPU)': 'Stuck process (% CPU)',
    'Editar limiares de alerta': 'Edit alert thresholds',
    'Grava a seção {s} de {f} (o resto do arquivo é preservado) e vale na hora.': 'Writes the {s} section of {f} (the rest of the file is kept) and applies immediately.',
    'Avisos do arquivo: {w}': 'File warnings: {w}', 'Limiares salvos.': 'Thresholds saved.', 'não configurado': 'not configured',
    'com token': 'with token', 'configurado': 'configured', 'Encaminhar alertas': 'Forward alerts',
    'Encaminhar "agente pedindo atenção"': 'Forward "agent needs attention"',
    'Segredos (tokens, URLs completas) ficam só no {f} (0600) e nunca voltam para a tela. O envio usa o {c}.': 'Secrets (tokens, full URLs) stay only in {f} (0600) and never come back to the screen. Sending uses {c}.',
    '•••• (configurado — deixe em branco para manter)': '•••• (configured — leave blank to keep)', 'Canais externos de notificação': 'External notification channels',
    'Envie alertas para o celular ou para um chat. Campos em branco mantêm o valor atual; marque “remover” para apagar um canal.': 'Send alerts to your phone or a chat. Blank fields keep the current value; tick “remove” to delete a channel.',
    'ntfy: URL do tópico': 'ntfy: topic URL', 'https://ntfy.sh/um-topico-dificil-de-adivinhar': 'https://ntfy.sh/a-hard-to-guess-topic',
    'ntfy: token (opcional)': 'ntfy: token (optional)', 'Webhook (JSON genérico ou Discord)': 'Webhook (generic JSON or Discord)',
    'Telegram: token do bot': 'Telegram: bot token', 'remover ntfy': 'remove ntfy', 'remover webhook': 'remove webhook',
    'remover Telegram': 'remove Telegram', 'encaminhar alertas do sistema': 'forward system alerts',
    'encaminhar quando um agente pedir atenção': 'forward when an agent needs attention',
    'Tópicos do ntfy.sh são públicos para quem souber o nome: use um nome longo e aleatório ou um servidor com token.': 'ntfy.sh topics are public to anyone who knows the name: use a long random name or a server with a token.',
    'Canais salvos.': 'Channels saved.', 'Nenhum canal configurado.': 'No channel configured.', 'Teste enviado: {c}': 'Test sent: {c}',
    'Ativos agora': 'Active now', 'Regras': 'Rules', 'Editar limiares': 'Edit thresholds', 'Notificação no desktop': 'Desktop notification',
    'Canais externos': 'External channels', 'Enviar teste': 'Send test',
    // atalhos e tema
    'Atalhos de teclado': 'Keyboard shortcuts', 'buscar/filtrar na tela atual': 'search/filter on the current screen',
    'alternar tema (auto → claro → escuro)': 'cycle theme (auto → light → dark)', 'esta ajuda': 'this help', 'fechar diálogos': 'close dialogs',
    'Tema: {t}': 'Theme: {t}', 'automático': 'automatic', 'claro': 'light', 'escuro': 'dark',
    // mensagens fixas do backend (api.rs, services.rs, smart.rs, schedule.rs, hooks.rs)
    'corpo não é UTF-8': 'body is not UTF-8', 'Host não permitido': 'Host not allowed', 'Origin não permitida': 'Origin not allowed',
    'autenticação necessária': 'authentication required', 'POST exige o cabeçalho X-Pulse-Token': 'POST requires the X-Pulse-Token header',
    'método não permitido': 'method not allowed', 'rota não encontrada': 'route not found',
    'máquinas remotas são somente leitura: abra o Pulse dela para executar ações': 'remote machines are read-only: open that machine\'s Pulse to run actions',
    'só rotas /api/ da outra máquina': 'only /api/ routes of the other machine', 'máquina não configurada': 'machine not configured',
    'range deve ser 1h, 6h, 24h, 7d, 30d ou 90d': 'range must be 1h, 6h, 24h, 7d, 30d or 90d', 'seconds deve ser de 1 a 30': 'seconds must be from 1 to 30',
    'a medição eBPF precisa do pulse-helper, instalado pelo pacote .deb': 'eBPF measurement needs pulse-helper, installed by the .deb package',
    'já há varreduras demais em andamento; aguarde': 'too many scans already running; please wait',
    'PID inválido': 'invalid PID', 'PID protegido': 'protected PID', 'o Pulse não age sobre si mesmo': 'Pulse does not act on itself',
    'processo não encontrado': 'process not found', 'ação desconhecida': 'unknown action', 'nice deve estar entre -20 e 19': 'nice must be between -20 and 19',
    'SIGTERM enviado; SIGKILL em 5 s se não encerrar': 'SIGTERM sent; SIGKILL in 5 s if it does not exit', 'SIGKILL enviado': 'SIGKILL sent',
    'processo pausado': 'process paused', 'processo retomado': 'process resumed',
    'ação inválida': 'invalid action', 'nome de unidade inválido': 'invalid unit name', 'unidade inválida': 'invalid unit', 'systemctl falhou': 'systemctl failed',
    'o pkexec não pode ganhar privilégio neste processo (serviço com NoNewPrivileges=yes?)': 'pkexec cannot gain privileges in this process (service with NoNewPrivileges=yes?)',
    'autenticação cancelada': 'authentication cancelled', 'não autorizado pelo polkit': 'not authorized by polkit',
    'nenhum agente de autenticação na sessão (a janela de senha não pôde ser exibida)': 'no authentication agent in the session (the password prompt could not be shown)',
    'a ação privilegiada falhou': 'the privileged action failed',
    'saída do smartctl não é JSON': 'smartctl output is not JSON',
    'sem permissão: o smartctl precisa de root (ou do grupo disk) para ler este disco': 'permission denied: smartctl needs root (or the disk group) to read this disk',
    'smartctl não está instalado (pacote smartmontools)': 'smartctl is not installed (smartmontools package)',
    'sem permissão no socket do Docker (adicione o usuário ao grupo docker)': 'no permission on the Docker socket (add the user to the docker group)',
    'Docker indisponível': 'Docker unavailable', 'identificador de contêiner inválido': 'invalid container identifier',
    'settings.json não é um objeto JSON': 'settings.json is not a JSON object', 'o campo "hooks" não é um objeto': 'the "hooks" field is not an object',
    'consumo da CPU (RAPL) só é legível pelo root neste kernel': 'CPU power draw (RAPL) is only readable by root on this kernel',
    'caminho precisa ser absoluto': 'path must be absolute', 'não é um diretório': 'not a directory',
    'Temporários': 'Temporary files', 'Arquivos temporários antigos (>7 dias)': 'Old temporary files (>7 days)', 'Cache do APT': 'APT cache',
    'Pacotes .deb baixados': 'Downloaded .deb packages', 'Journal do systemd': 'systemd journal', 'Logs persistentes do journald': 'Persistent journald logs',
    'Cache do usuário': 'User cache', 'Caches de aplicativos (recriados sob demanda)': 'Application caches (recreated on demand)',
    'Lixeira': 'Trash', 'Itens na lixeira': 'Items in the trash', 'Snaps antigos': 'Old snaps', 'Revisões desativadas de snaps': 'Disabled snap revisions',
    'CPU alta': 'High CPU', 'Memória alta': 'High memory', 'Memória crítica': 'Critical memory', 'Swap alta': 'High swap',
    'Disco quase cheio': 'Disk almost full', 'Disco cheio': 'Disk full', 'Temperatura alta': 'High temperature', 'Processo preso': 'Stuck process',
  };

  async function boot() {
    applyStatic();
    renderNav();
    const m = location.hash.match(/^#token=([0-9a-fA-F]{32,})/);
    if (m) {
      history.replaceState(null, '', location.pathname + '#/visao-geral');
      try { await login(m[1]); } catch { toast(t('Token da URL inválido.'), true); }
    }
    let h = null;
    try { h = await (await fetch('/api/health', { credentials: 'same-origin' })).json(); } catch { /* fora do ar */ }
    if (!h) { $('#page-sub').textContent = t('Daemon indisponível.'); return; }
    if (!h.authenticated) {
      if (token()) { try { await login(token()); } catch { session.del('pulse.token'); } }
      h = await (await fetch('/api/health', { credentials: 'same-origin' })).json();
      if (!h.authenticated) { route(); await showLogin(); return; }
    }
    await loadRemotes();
    document.documentElement.classList.toggle('remote', !!S.remote);
    await loadInitial();
    route();
    startStream();
    setInterval(loadRemotes, 30000);
  }
  boot();
})();
