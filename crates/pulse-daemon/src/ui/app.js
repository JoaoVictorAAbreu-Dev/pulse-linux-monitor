/* Pulse — interface (JS puro, sem build). Todo dado dinâmico passa por esc(). */
(() => {
  'use strict';

  // ------------------------------------------------------------ utilidades
  const $ = (s, r = document) => r.querySelector(s);
  const $$ = (s, r = document) => Array.from(r.querySelectorAll(s));
  const ESC = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' };
  const esc = (s) => String(s ?? '').replace(/[&<>"']/g, (c) => ESC[c]);
  const fmtCache = {};
  const nf = (d) => (fmtCache[d] ||= new Intl.NumberFormat('pt-BR', { minimumFractionDigits: d, maximumFractionDigits: d }));
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
    if (s < 5) return 'agora';
    if (s < 60) return `há ${Math.floor(s)}s`;
    if (s < 3600) return `há ${Math.floor(s / 60)}m`;
    if (s < 86400) return `há ${Math.floor(s / 3600)}h`;
    return `há ${Math.floor(s / 86400)}d`;
  }
  const clock = (ms) => new Date(ms).toLocaleTimeString('pt-BR');
  const dateTime = (ms) => new Date(ms).toLocaleString('pt-BR', { dateStyle: 'short', timeStyle: 'medium' });
  const lvl = (v, w, c) => (v > c ? 'crit' : v > w ? 'warn' : '');
  const store = {
    get(k) { try { return localStorage.getItem(k); } catch { return null; } },
    set(k, v) { try { localStorage.setItem(k, v); } catch { /* sem armazenamento */ } },
  };
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
    hist: { ts: [], cpu: [], mem: [], swap: [], rx: [], tx: [], rd: [], wr: [], temp: [] },
    lastSeq: 0,
    page: null,
    agents: null,
    es: null,
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
    };
    for (const k in h) { h[k].push(p[k]); if (h[k].length > HIST_MAX) h[k].shift(); }
  }
  const maxTemp = (s) => (s.temps && s.temps.length ? Math.max(...s.temps.map((t) => t.c)) : null);

  // ------------------------------------------------------------ API
  const token = () => session.get('pulse.token');
  async function api(path, opts = {}) {
    const headers = {};
    const method = opts.method || 'GET';
    if (method !== 'GET') {
      if (!token()) await askToken('Para executar ações, informe o token de acesso.');
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
      throw new Error((j && j.error) || `HTTP ${r.status}`);
    }
    return j;
  }

  async function login(t) {
    const r = await fetch('/api/login', { method: 'POST', headers: { 'X-Pulse-Token': t }, credentials: 'same-origin' });
    if (!r.ok) throw new Error('token inválido');
    session.set('pulse.token', t);
  }

  // ------------------------------------------------------------ diálogos
  let dialogResolve = null;
  function dialog({ title, body, actions }) {
    const ov = $('#overlay');
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
    dialog({ title, body: html, actions: [{ label: 'Cancelar', value: false }, { label, cls, value: true }] });

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
        title: 'Acesso ao Pulse',
        body: `<p class="sub">${esc(msg)}</p>
          <label class="field">Token<input id="tok" class="input mono" type="password" autocomplete="off" spellcheck="false" placeholder="cole o token aqui"></label>
          <p class="muted">Obtenha a URL de acesso com <code class="cmd">pulse --print-token</code> ou leia <code class="cmd">~/.config/pulse/token</code>.</p>`,
        actions: [{ label: 'Entrar', cls: 'primary', onclick: () => closeDialog($('#tok').value.trim()) }],
      });
      if (!ok) { if (token()) return; continue; }
      try { await login(ok); toast('Sessão iniciada.'); return; } catch { toast('Token inválido.', true); }
    }
  }
  let loginShown = false;
  async function showLogin() {
    if (loginShown) return;
    loginShown = true;
    await askToken('Informe o token de acesso para ver o painel.');
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
      paths += `<path d="${d}" fill="none" stroke="${s.color}" stroke-width="2" stroke-linejoin="round" stroke-linecap="round"/>`;
    }
    const first = ts && ts.length ? ts[0] : null;
    const leftLbl = first && spanLabel === 'auto' ? dateTime(first) : spanLabel;
    g += `<text x="${L}" y="${H - 4}">${esc(leftLbl)}</text><text x="${W - R}" y="${H - 4}" text-anchor="end">agora</text>`;
    el.innerHTML = `<svg class="chart" viewBox="0 0 ${W} ${H}" width="${W}" height="${H}" role="img" aria-label="gráfico">${g}${paths}</svg>`;
  }

  // ------------------------------------------------------------ páginas
  const PAGES = [];
  const page = (id, title, icon, def) => PAGES.push({ id, title, icon, ...def });

  const card = (title, body, extra = '') => `<section class="card glass">${title ? `<h2>${title}${extra}</h2>` : ''}${body}</section>`;
  const panel = (title, body, extra = '') => `<section class="panel glass">${title ? `<h2>${title}${extra}</h2>` : ''}${body}</section>`;
  const kpi = (id, ringHtml = '') => `<section class="card glass kpi" id="${id}">${ringHtml}<div><div class="lbl"></div><div class="val"></div><div class="det"></div></div></section>`;
  function setKpi(id, label, val, det, ringSvg) {
    const el = document.getElementById(id);
    if (!el) return;
    const r = el.querySelector('.ring');
    if (ringSvg !== undefined) {
      if (r) r.outerHTML = ringSvg; else el.insertAdjacentHTML('afterbegin', ringSvg);
    }
    el.querySelector('.lbl').textContent = label;
    el.querySelector('.val').textContent = val;
    el.querySelector('.det').textContent = det;
  }
  const html = (id, h) => { const el = document.getElementById(id); if (el) el.innerHTML = h; };
  const barHtml = (v, cls = '') => `<div class="bar"><i class="${cls}" style="width:${Math.max(0, Math.min(100, v || 0)).toFixed(1)}%"></i></div>`;
  const pill = (t, cls = '') => `<span class="pill ${cls}">${esc(t)}</span>`;
  const sevPill = (s) => (s === 'critico' ? pill('Crítico', 'crit') : pill('Aviso', 'warn'));
  const rootFs = (s) => s.disks.fs.find((f) => f.mount === '/') || s.disks.fs[0];

  function consumersTable(list, memFirst) {
    if (!list || !list.length) return '<p class="empty">Sem dados.</p>';
    return `<div class="table-wrap"><table><thead><tr><th>Processo</th><th class="num">CPU</th><th class="num">Memória</th></tr></thead><tbody>
      ${list.map((p) => `<tr><td class="trunc-s" title="${esc(p.cmd)}">${esc(p.name)} <span class="muted">· ${esc(p.pid)}</span></td><td class="num">${pct(p.cpu)}</td><td class="num">${memFirst ? '<b>' + bytes(p.rss) + '</b>' : bytes(p.rss)}</td></tr>`).join('')}
    </tbody></table></div>`;
  }

  // ---- 1. Visão geral
  let longRange = store.get('pulse.range') || '15m';
  let longData = null;
  let longTimer = 0;
  async function loadLong() {
    if (longRange === '15m') { longData = null; return; }
    try { longData = await api('/api/history/long?range=' + longRange); } catch (e) { toast('Histórico longo: ' + e.message, true); }
  }
  function drawMainChart() {
    const el = $('#ov-chart');
    if (!el) return;
    if (longRange === '15m' || !longData) {
      chart(el, { series: [{ data: S.hist.cpu, color: 'var(--acento)' }, { data: S.hist.mem, color: 'var(--violeta)' }], ts: S.hist.ts, max: 100 });
    } else {
      const p = longData.points;
      chart(el, { series: [{ data: p.cpu, color: 'var(--acento)' }, { data: p.mem, color: 'var(--violeta)' }], ts: p.ts, max: 100, spanLabel: p.ts.length ? 'auto' : '—' });
    }
    const n = longRange === '15m' ? S.hist.ts.length : longData ? longData.count : 0;
    html('ov-chart-note', longRange === '15m' ? `${n} amostras de 1 s` : `${n} médias de 60 s (histórico longo)`);
  }

  page('visao-geral', 'Visão geral', 'grid', {
    mount(el) {
      el.innerHTML = `
        <div class="grid g3">${kpi('k-cpu', ring(0, 'var(--acento)'))}${kpi('k-mem', ring(0, 'var(--violeta)'))}${kpi('k-swap', ring(0, 'var(--violeta)'))}</div>
        <div class="grid g3">${kpi('k-disk', ring(0, 'var(--acento)'))}${kpi('k-net')}${kpi('k-temp')}</div>
        <div class="grid g-main">
          ${card('CPU e memória', `<div id="ov-chart"></div><div class="toolbar"><div class="series-legend"><span><i class="dot" style="background:var(--acento)"></i>CPU</span><span><i class="dot" style="background:var(--violeta)"></i>Memória</span></div><span class="sp"></span><span class="muted" id="ov-chart-note"></span></div>`,
            `<span class="sp"></span><span class="seg" id="ov-range">${['15m', '1h', '24h', '7d'].map((r) => `<button data-r="${r}" aria-pressed="${r === longRange}">${r === '15m' ? '15 min' : r.replace('h', ' h').replace('d', ' d')}</button>`).join('')}</span>`)}
          ${card('Alertas', '<div id="ov-alerts"></div>')}
        </div>
        <div class="grid g2">
          ${card('Maiores consumidores', '<div id="ov-top"></div>')}
          ${card('Agentes agora', '<div id="ov-agents"><p class="empty">Carregando…</p></div>', '<span class="sp"></span><a class="chip" href="#/agentes">ver agentes</a>')}
        </div>
        ${card('Armazenamento', '<div id="ov-storage"></div>')}`;
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
      setKpi('k-cpu', 'CPU', pct(s.cpu.total), `carga ${num(s.cpu.load[0], 2)} · ${s.host.cores} núcleos`, ring(s.cpu.total, 'var(--acento)'));
      setKpi('k-mem', 'Memória', bytes(s.mem.used), `de ${bytes(s.mem.total)} · ${pct(s.mem.pct, 0)}`, ring(s.mem.pct, 'var(--violeta)'));
      setKpi('k-swap', 'Swap', s.mem.swap_total ? bytes(s.mem.swap_used) : '—', s.mem.swap_total ? `de ${bytes(s.mem.swap_total)}` : 'swap desativada', ring(s.mem.swap_pct, 'var(--violeta)'));
      if (fs) setKpi('k-disk', 'Disco', pct(fs.pct, 0), `${bytes(fs.avail)} livres em ${fs.mount}`, ring(fs.pct, fs.pct > 90 ? 'var(--aviso)' : 'var(--acento)'));
      setKpi('k-net', 'Rede', '↓ ' + rate(s.net.rx_bps), '↑ ' + rate(s.net.tx_bps));
      const t = maxTemp(s);
      const hot = s.temps.slice().sort((a, b) => b.c - a.c)[0];
      setKpi('k-temp', 'Temperatura', t == null ? '—' : num(t, 0) + ' °C', hot ? `${hot.chip} · ${hot.label}` : 'sem sensores disponíveis');
      if (longRange === '15m') drawMainChart();
      html('ov-alerts', s.alerts.length
        ? s.alerts.map((a) => `<div class="toolbar" style="margin-bottom:8px">${sevPill(a.severity)}<b>${esc(a.title)}</b><span class="sub">${esc(a.detail)}</span></div>`).join('')
        : '<p class="empty">Nenhum alerta ativo.</p>');
      html('ov-top', consumersTable(s.procs.top.slice(0, 6)));
      html('ov-storage', s.disks.fs.map((f) => `
        <div class="toolbar" style="margin-bottom:12px;flex-wrap:nowrap">
          <div style="width:200px;min-width:0"><div class="trunc-s"><b>${esc(f.mount)}</b></div><div class="muted" style="font-size:12px">${esc(f.fstype)} · ${esc(f.device)}</div></div>
          <div style="flex:1">${barHtml(f.pct, lvl(f.pct, 90, 97))}</div>
          <div class="sub num" style="width:190px">${bytes(f.used)} / ${bytes(f.total)} · ${pct(f.pct, 0)}</div>
        </div>`).join('') || '<p class="empty">Nenhum sistema de arquivos.</p>');
      renderAgentsNow();
    },
  });

  const HOOK_STATE = {
    executando: ['Executando', 'ok'],
    atencao: ['Pedindo atenção', 'warn'],
    aguardando: ['Aguardando', ''],
    concluido: ['Concluído', 'info'],
  };
  const statePill = (st) => { const [t, c] = HOOK_STATE[st] || [st, '']; return pill(t, c); };

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
    if (!S.agents.enabled) { el.innerHTML = '<p class="empty">Detecção de agentes desativada (--no-agents).</p>'; return; }
    const hs = S.agents.hooks.sessions.filter((h) => Date.now() - h.ts < 6 * 3600e3).slice(0, 4);
    const ps = S.agents.processes;
    const sess = S.agents.sessions.slice(0, 3);
    let out = '';
    if (hs.length) out += hs.map((h) => `<div class="toolbar" style="margin-bottom:8px"><b>${esc(agentName(h.agent))}</b>${statePill(h.state)}<span class="sub">${esc(h.tool || h.event)} · ${ago(h.ts)}</span></div>`).join('');
    if (sess.length) out += sess.map((x) => `<div class="toolbar" style="margin-bottom:8px"><b>${esc(x.agent)}</b><span class="sub trunc-s">${esc(x.project_name || x.project)}</span><span class="muted">${esc(x.model || '')} · ${ago(x.updated)}</span></div>`).join('');
    if (!out) out = '<p class="empty">Nenhuma sessão recente.</p>';
    out += `<p class="sub">${ps.length} processo${ps.length === 1 ? '' : 's'} de agente em execução${ps.length ? ': ' + esc([...new Set(ps.map((p) => p.agent))].join(', ')) : ''}.</p>`;
    el.innerHTML = out;
  }
  const agentName = (a) => ({ claude: 'Claude Code', codex: 'Codex', openclaw: 'OpenClaw', ponto: 'Ponto' })[String(a).toLowerCase()] || a;

  // ---- 2. CPU
  page('cpu', 'CPU', 'cpu', {
    mount(el) {
      el.innerHTML = `
        <div class="grid g-main">${card('Histórico', '<div id="cpu-chart"></div>')}${card('Detalhes', '<dl class="kv" id="cpu-kv"></dl>')}</div>
        ${panel('Núcleos', '<div class="tiles" id="cpu-cores"></div>', '<span class="sp"></span><span class="muted" id="cpu-model" style="text-transform:none;letter-spacing:0;font-weight:500"></span>')}
        ${panel('Temperaturas', '<div class="tiles" id="cpu-temps"></div>')}`;
    },
    update(s) {
      const c = s.cpu;
      chart($('#cpu-chart'), { series: [{ data: S.hist.cpu, color: 'var(--acento)' }], ts: S.hist.ts, max: 100, height: 220 });
      const psi = c.psi ? `${pct(c.psi.some10)} (10 s)` : 'indisponível';
      html('cpu-kv', [
        ['Uso total', pct(c.total)], ['Usuário', pct(c.user)], ['Sistema', pct(c.system)], ['I/O wait', pct(c.iowait)],
        ['IRQ / steal', `${pct(c.irq)} · ${pct(c.steal)}`],
        ['Carga 1/5/15 min', c.load.map((v) => num(v, 2)).join(' · ')], ['Executando / bloqueados', `${c.procs_running} / ${c.procs_blocked}`],
        ['Trocas de contexto', kilo(c.ctxt_ps) + '/s'], ['Pressão (PSI)', psi], ['Governor', c.governor || '—'],
      ].map(([k, v]) => `<dt>${esc(k)}</dt><dd>${esc(v)}</dd>`).join(''));
      $('#cpu-model').textContent = c.model || '';
      html('cpu-cores', c.cores.map((k) => `<div class="tile"><div class="t">núcleo ${k.id}${k.mhz ? ' · ' + num(k.mhz, 0) + ' MHz' : ''}</div><div class="v">${pct(k.pct, 0)}</div>${barHtml(k.pct, lvl(k.pct, 70, 90))}</div>`).join(''));
      html('cpu-temps', s.temps.length
        ? s.temps.map((t) => { const ref = t.crit || 100; return `<div class="tile"><div class="t" title="${esc(t.chip)}">${esc(t.chip)} · ${esc(t.label)}</div><div class="v">${num(t.c, 0)} °C</div>${barHtml((t.c / ref) * 100, t.c > 88 ? 'crit' : t.c > 75 ? 'warn' : 'ok')}</div>`; }).join('')
        : '<p class="empty">Nenhum sensor de temperatura disponível.</p>');
    },
  });

  // ---- 3. Memória
  page('memoria', 'Memória', 'mem', {
    mount(el) {
      el.innerHTML = `
        <div class="grid g4">${kpi('m-used')}${kpi('m-avail')}${kpi('m-cache')}${kpi('m-swap')}</div>
        ${panel('Composição', '<div class="stack" id="m-stack"></div><div class="legend" id="m-legend"></div>')}
        <div class="grid g-main">
          ${card('Histórico', '<div id="m-chart"></div><div class="series-legend"><span><i class="dot" style="background:var(--violeta)"></i>Memória</span><span><i class="dot" style="background:var(--aviso)"></i>Swap</span></div>')}
          ${card('Detalhes', '<dl class="kv" id="m-kv"></dl>')}
        </div>
        ${card('Maiores consumidores de memória', '<div id="m-top"></div>')}`;
    },
    update(s) {
      const m = s.mem;
      setKpi('m-used', 'Em uso', bytes(m.used), `${pct(m.pct)} de ${bytes(m.total)}`);
      setKpi('m-avail', 'Disponível', bytes(m.available), 'livre + cache recuperável');
      setKpi('m-cache', 'Cache', bytes(m.cache), `buffers ${bytes(m.buffers)}`);
      setKpi('m-swap', 'Swap', m.swap_total ? bytes(m.swap_used) : '—', m.swap_total ? `${pct(m.swap_pct)} de ${bytes(m.swap_total)}` : 'swap desativada');
      const t = m.total || 1;
      const parts = [['Aplicativos', m.apps, 'var(--acento)'], ['Cache', m.cache, 'var(--violeta)'], ['Livre', m.free, 'transparent']];
      html('m-stack', parts.map(([, v, c]) => `<i style="width:${((v / t) * 100).toFixed(2)}%;background:${c}"></i>`).join(''));
      html('m-legend', parts.map(([n, v, c]) => `<span><i class="dot" style="background:${c === 'transparent' ? 'var(--trilho)' : c}"></i>${n} <b>${bytes(v)}</b> · ${pct((v / t) * 100, 0)}</span>`).join(''));
      chart($('#m-chart'), { series: [{ data: S.hist.mem, color: 'var(--violeta)' }, { data: S.hist.swap, color: 'var(--aviso)', fill: false }], ts: S.hist.ts, max: 100 });
      const psi = m.psi ? `${pct(m.psi.some10)} · total ${pct(m.psi.full10 ?? 0)}` : 'indisponível';
      html('m-kv', [
        ['Memória compartilhada (shmem)', bytes(m.shmem)], ['Páginas sujas', bytes(m.dirty)], ['Em writeback', bytes(m.writeback)],
        ['Swap em cache', bytes(m.swap_cached)], ['Swap in / out', `${num(m.swap_in_ps, 0)} / ${num(m.swap_out_ps, 0)} pág/s`],
        ['Faltas de página maiores', num(m.major_faults_ps, 0) + '/s'], ['Swappiness', m.swappiness ?? '—'],
        ['Mortes por OOM (desde o boot)', num(m.oom_kills)], ['Pressão (PSI) 10 s', psi],
      ].map(([k, v]) => `<dt>${esc(k)}</dt><dd>${esc(v)}</dd>`).join(''));
      html('m-top', consumersTable(s.procs.top_mem, true));
    },
  });

  // ---- 4. Discos
  let duPath = store.get('pulse.du') || '';
  async function loadDu(path) {
    html('du-body', '<p class="empty">Calculando… (pode levar alguns segundos)</p>');
    try {
      const r = await api('/api/disk/usage' + (path ? '?path=' + encodeURIComponent(path) : ''));
      duPath = r.path;
      store.set('pulse.du', duPath);
      const parts = r.path.split('/').filter(Boolean);
      let acc = '';
      const crumbs = [`<button data-p="/">/</button>`].concat(parts.map((p) => { acc += '/' + p; return `<button data-p="${esc(acc)}">${esc(p)}</button>`; }));
      html('du-crumbs', crumbs.join('<span class="muted">›</span>') + `<span class="sp"></span><span class="muted">total ${bytes(r.total)}${r.truncated ? ' · varredura parcial (limite de tempo/arquivos)' : ''}${r.errors ? ` · ${r.errors} sem permissão` : ''}</span>`);
      const max = r.entries.length ? r.entries[0].size : 1;
      html('du-body', r.entries.length ? `<div class="table-wrap tall"><table><thead><tr><th>Nome</th><th style="width:40%">Proporção</th><th class="num">Tamanho</th></tr></thead><tbody>
        ${r.entries.map((e) => `<tr class="${e.dir ? 'clickable' : ''}" ${e.dir ? `data-p="${esc(e.path)}" tabindex="0"` : ''}><td class="trunc">${e.dir ? '📁 ' : ''}${esc(e.name)}</td><td>${barHtml((e.size / (max || 1)) * 100)}</td><td class="num">${bytes(e.size)}</td></tr>`).join('')}
        </tbody></table></div>` : '<p class="empty">Diretório vazio.</p>');
    } catch (e) {
      html('du-body', `<p class="empty">Erro: ${esc(e.message)}</p>`);
    }
  }
  page('discos', 'Discos', 'disk', {
    mount(el) {
      el.innerHTML = `
        ${card('Sistemas de arquivos', '<div id="d-fs"></div>')}
        <div class="grid g-main">
          ${card('Throughput', '<div id="d-chart"></div><div class="series-legend"><span><i class="dot" style="background:var(--acento)"></i>Leitura</span><span><i class="dot" style="background:var(--violeta)"></i>Escrita</span></div>')}
          ${card('Pressão de E/S', '<dl class="kv" id="d-kv"></dl>')}
        </div>
        ${card('Dispositivos', '<div id="d-dev"></div>')}
        ${panel('O que ocupa espaço?', '<div class="crumbs toolbar" id="du-crumbs"></div><div id="du-body" style="margin-top:10px"></div>', '<span class="sp"></span><button class="btn sm" id="du-home">Pasta pessoal</button><button class="btn sm" id="du-reload">Atualizar</button>')}
        ${panel('Candidatos à limpeza', '<p class="sub">O Pulse <b>nunca executa</b> estes comandos — só sugere. Revise antes de rodar no terminal.</p><div id="d-clean"><p class="empty">Calculando tamanhos…</p></div>')}`;
      const nav = (e) => { const t = e.target.closest('[data-p]'); if (t) loadDu(t.dataset.p); };
      $('#du-crumbs').addEventListener('click', nav);
      $('#du-body').addEventListener('click', nav);
      $('#du-body').addEventListener('keydown', (e) => { if (e.key === 'Enter') nav(e); });
      $('#du-home').addEventListener('click', () => loadDu(''));
      $('#du-reload').addEventListener('click', () => loadDu(duPath));
      loadDu(duPath);
      api('/api/disk/cleanup').then((list) => {
        html('d-clean', list.length ? `<div class="table-wrap"><table><thead><tr><th>Item</th><th>Caminho</th><th class="num">Tamanho</th><th>Comando sugerido</th></tr></thead><tbody>
          ${list.map((c) => `<tr><td><b>${esc(c.title)}</b><div class="muted" style="font-size:12px">${esc(c.description)}</div></td><td class="mono trunc-s">${esc(c.path)}</td><td class="num">${c.size == null ? '—' : bytes(c.size)}</td><td><code class="cmd">${esc(c.command)}</code></td></tr>`).join('')}
          </tbody></table></div>` : '<p class="empty">Nada a sugerir.</p>');
      }).catch((e) => html('d-clean', `<p class="empty">Erro: ${esc(e.message)}</p>`));
    },
    update(s) {
      html('d-fs', `<div class="table-wrap"><table><thead><tr><th>Montagem</th><th>Dispositivo</th><th>Tipo</th><th style="width:28%">Uso</th><th class="num">Usado / total</th><th class="num">Livre</th><th class="num">Inodes</th></tr></thead><tbody>
        ${s.disks.fs.map((f) => `<tr><td><b>${esc(f.mount)}</b></td><td class="mono">${esc(f.device)}</td><td>${esc(f.fstype)}</td><td>${barHtml(f.pct, lvl(f.pct, 90, 97))}</td><td class="num">${bytes(f.used)} / ${bytes(f.total)} · ${pct(f.pct, 0)}</td><td class="num">${bytes(f.avail)}</td><td class="num">${pct(f.inodes_pct, 0)}</td></tr>`).join('')}
        </tbody></table></div>`);
      chart($('#d-chart'), { series: [{ data: S.hist.rd, color: 'var(--acento)' }, { data: S.hist.wr, color: 'var(--violeta)' }], ts: S.hist.ts, max: 'auto', fmt: (v) => rate(v) });
      const p = s.disks.psi;
      html('d-kv', [['Leitura agora', rate(s.disks.read_bps)], ['Escrita agora', rate(s.disks.write_bps)],
        ['PSI some (10 s)', p ? pct(p.some10) : '—'], ['PSI full (10 s)', p && p.full10 != null ? pct(p.full10) : '—'],
        ['PSI some (60 s)', p ? pct(p.some60) : '—']].map(([k, v]) => `<dt>${esc(k)}</dt><dd>${esc(v)}</dd>`).join(''));
      html('d-dev', s.disks.devices.length ? `<div class="table-wrap"><table><thead><tr><th>Dispositivo</th><th>Modelo</th><th class="num">Tamanho</th><th class="num">Leitura</th><th class="num">Escrita</th><th class="num">IOPS (l/e)</th><th style="width:20%">Ocupação</th></tr></thead><tbody>
        ${s.disks.devices.map((d) => `<tr><td><b>${esc(d.name)}</b> <span class="muted">${d.rotational ? 'HDD' : 'SSD'}</span></td><td class="trunc-s">${esc(d.model || '—')}</td><td class="num">${bytes(d.size)}</td><td class="num">${rate(d.read_bps)}</td><td class="num">${rate(d.write_bps)}</td><td class="num">${num(d.read_iops, 0)} / ${num(d.write_iops, 0)}</td><td><div class="toolbar" style="flex-wrap:nowrap"><div style="flex:1">${barHtml(d.busy_pct, lvl(d.busy_pct, 70, 90))}</div><span class="num sub">${pct(d.busy_pct, 0)}</span></div></td></tr>`).join('')}
        </tbody></table></div>` : '<p class="empty">Nenhum dispositivo de bloco.</p>');
    },
  });

  // ---- 5. Processos
  const P = { list: [], sort: { k: 'cpu', dir: -1 }, filter: '', mine: false, timer: 0 };
  const STATE_NAMES = { R: 'rodando', S: 'dormindo', D: 'E/S', I: 'ocioso', Z: 'zumbi', T: 'parado', t: 'rastreado', X: 'morto' };
  async function loadProcs() {
    clearTimeout(P.timer);
    try {
      const r = await api('/api/processes');
      P.list = r.list;
      const s = r.summary;
      html('p-summary', [['Total', s.total], ['Rodando', s.running], ['Dormindo', s.sleeping], ['Zumbis', s.zombie], ['Parados', s.stopped], ['Threads', s.threads]]
        .map(([k, v]) => `<span class="chip">${k} <b>${num(v)}</b></span>`).join(''));
      renderProcs();
    } catch { /* tenta de novo */ }
    if (S.page && S.page.id === 'processos') P.timer = setTimeout(loadProcs, 2000);
  }
  function renderProcs() {
    const f = P.filter.toLowerCase();
    let rows = P.list.filter((p) => !f || p.cmd.toLowerCase().includes(f) || p.name.toLowerCase().includes(f) || String(p.pid) === f || p.user.toLowerCase() === f);
    if (P.mine && S.user) rows = rows.filter((p) => p.user === S.user);
    const { k, dir } = P.sort;
    rows.sort((a, b) => (a[k] < b[k] ? -dir : a[k] > b[k] ? dir : 0));
    const total = rows.length;
    rows = rows.slice(0, 300);
    $('#p-count').textContent = total > 300 ? `mostrando 300 de ${total}` : `${total} processos`;
    const cols = [['pid', 'PID', 1], ['user', 'Usuário'], ['state', 'Estado'], ['cpu', 'CPU', 1], ['rss', 'RAM', 1], ['threads', 'Threads', 1], ['nice', 'Nice', 1], ['cmd', 'Comando']];
    html('p-table', `<table class="t-compact"><thead><tr>${cols.map(([c, t, n]) => `<th class="sortable ${n ? 'num' : ''}" data-k="${c}" aria-sort="${P.sort.k === c ? (dir > 0 ? 'ascending' : 'descending') : 'none'}">${t}</th>`).join('')}<th class="num">Ações</th></tr></thead><tbody>
      ${rows.map((p) => `<tr><td class="num">${p.pid}</td><td>${esc(p.user)}</td><td>${pill(STATE_NAMES[p.state] || p.state, p.state === 'R' ? 'ok' : p.state === 'Z' ? 'crit' : p.state === 'T' ? 'warn' : '')}</td>
        <td class="num">${pct(p.cpu)}</td><td class="num">${bytes(p.rss)}</td><td class="num">${p.threads}</td><td class="num">${p.nice}</td>
        <td class="trunc" title="${esc(p.cmd)}"><b>${esc(p.name)}</b> <span class="muted">${esc(p.cmd)}</span></td>
        <td><div class="actions" data-pid="${p.pid}" data-name="${esc(p.name)}">
          <button class="btn sm" data-a="${p.state === 'T' ? 'resume' : 'pause'}">${p.state === 'T' ? 'Continuar' : 'Pausar'}</button>
          <button class="btn sm" data-a="renice" data-nice="${p.nice}">Prioridade</button>
          <button class="btn sm danger" data-a="terminate">Encerrar</button></div></td></tr>`).join('')}
      </tbody></table>`);
  }
  async function procAction(pid, name, action, curNice) {
    const label = `<b>${esc(name)}</b> (PID ${esc(pid)})`;
    let body;
    if (action === 'terminate') {
      if (!(await confirmAction('Encerrar processo?', `<p>Enviar SIGTERM para ${label}? Se não encerrar em 5 s, o Pulse envia SIGKILL.</p><p class="muted">A ação é registrada no log de auditoria.</p>`, 'Encerrar'))) return;
    } else if (action === 'pause') {
      if (!(await confirmAction('Pausar processo?', `<p>Enviar SIGSTOP para ${label}? Ele fica congelado até ser continuado.</p>`, 'Pausar', 'primary'))) return;
    } else if (action === 'resume') {
      if (!(await confirmAction('Continuar processo?', `<p>Enviar SIGCONT para ${label}?</p>`, 'Continuar', 'primary'))) return;
    } else if (action === 'renice') {
      const v = await dialog({
        title: 'Ajustar prioridade',
        body: `<p>Novo valor de <i>nice</i> para ${label} (−20 = maior prioridade, 19 = menor). Diminuir o nice exige privilégios.</p>
          <label class="field">Nice<input id="nice" class="input" type="number" min="-20" max="19" step="1" value="${esc(curNice ?? 0)}"></label>`,
        actions: [{ label: 'Cancelar', value: null }, { label: 'Aplicar', cls: 'primary', onclick: () => closeDialog($('#nice').value) }],
      });
      if (v == null || v === '') return;
      body = { nice: Number(v) };
    }
    try {
      const r = await api(`/api/processes/${encodeURIComponent(pid)}/${action}`, { method: 'POST', body: body || {} });
      toast(r.message || 'Feito.');
    } catch (e) { toast('Falhou: ' + e.message, true); }
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
            <input id="p-filter" class="input" type="search" placeholder="Filtrar por nome, comando, PID ou usuário" style="flex:1;max-width:420px" value="${esc(P.filter)}">
            <label class="chk"><input type="checkbox" id="p-mine" ${P.mine ? 'checked' : ''}> só meus</label>
            <span class="sp"></span><span class="muted" id="p-count"></span></div>
          <div class="table-wrap tall" id="p-table"><p class="empty">Carregando…</p></div>`)}`;
      $('#p-filter').addEventListener('input', (e) => { P.filter = e.target.value; renderProcs(); });
      $('#p-mine').addEventListener('change', (e) => { P.mine = e.target.checked; renderProcs(); });
      $('#p-table').addEventListener('click', (e) => {
        const th = e.target.closest('th[data-k]');
        if (!th) return;
        const k = th.dataset.k;
        P.sort = { k, dir: P.sort.k === k ? -P.sort.dir : (k === 'cpu' || k === 'rss' || k === 'threads' ? -1 : 1) };
        renderProcs();
      });
      loadProcs();
    },
    unmount() { clearTimeout(P.timer); },
  });

  // ---- 6. Serviços
  const SV = { scope: 'user', filter: 'todos', q: '', units: [], timer: 0 };
  async function loadServices() {
    clearTimeout(SV.timer);
    try {
      const r = await api('/api/services?scope=' + SV.scope);
      SV.units = r.units;
      renderServices();
    } catch (e) { html('sv-table', `<p class="empty">Erro: ${esc(e.message)}</p>`); }
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
    $$('#sv-filter button').forEach((b) => { b.textContent = `${b.dataset.label} (${counts[b.dataset.f]})`; });
    const st = (u) => (u.active === 'active' ? pill(u.sub === 'running' ? 'rodando' : u.sub, 'ok') : u.active === 'failed' ? pill('falhou', 'crit') : u.active === 'activating' ? pill('iniciando', 'info') : pill(u.active === 'inactive' ? 'inativo' : u.active));
    html('sv-table', rows.length ? `<table><thead><tr><th>Unidade</th><th>Descrição</th><th>Estado</th><th>Habilitado</th><th class="num">Ações</th></tr></thead><tbody>
      ${rows.map((u) => `<tr><td class="trunc-s" title="${esc(u.name)}"><b>${esc(u.name.replace(/\.service$/, ''))}</b></td><td class="trunc">${esc(u.description)}</td><td>${st(u)}</td><td>${esc(u.enabled || '—')}</td>
        <td><div class="actions" data-unit="${esc(u.name)}">
          ${u.active === 'active' ? '<button class="btn sm" data-v="restart">Reiniciar</button><button class="btn sm danger" data-v="stop">Parar</button>' : '<button class="btn sm primary" data-v="start">Iniciar</button>'}
        </div></td></tr>`).join('')}</tbody></table>` : '<p class="empty">Nenhum serviço corresponde ao filtro.</p>');
  }
  const fmtWhen = (ms) => (ms ? `${dateTime(ms)} (${ms > Date.now() ? 'em ' + dur((ms - Date.now()) / 1000) : ago(ms)})` : '—');
  async function loadTimers() {
    try {
      const r = await api('/api/timers?scope=' + SV.scope);
      html('sv-timers', r.timers.length ? `<div class="table-wrap"><table><thead><tr><th>Timer</th><th>Ativa</th><th>Próxima execução</th><th>Última execução</th></tr></thead><tbody>
        ${r.timers.map((t) => `<tr><td><b>${esc(t.unit.replace(/\.timer$/, ''))}</b></td><td class="trunc-s">${esc(t.activates)}</td><td>${esc(fmtWhen(t.next))}</td><td>${esc(fmtWhen(t.last))}</td></tr>`).join('')}</tbody></table></div>` : '<p class="empty">Nenhum timer neste escopo.</p>');
    } catch (e) { html('sv-timers', `<p class="empty">Erro: ${esc(e.message)}</p>`); }
  }
  async function loadCron() {
    try {
      const r = await api('/api/cron');
      const per = r.periodic.filter((p) => p.scripts.length).map((p) => `<span class="chip">${esc({ hourly: 'a cada hora', daily: 'diário', weekly: 'semanal', monthly: 'mensal' }[p.period] || p.period)}: <b>${esc(p.scripts.join(', '))}</b></span>`).join(' ');
      html('sv-cron', (r.entries.length ? `<div class="table-wrap"><table><thead><tr><th>Agenda</th><th>Usuário</th><th>Comando</th><th>Origem</th></tr></thead><tbody>
        ${r.entries.map((c) => `<tr><td class="mono">${esc(c.schedule)}</td><td>${esc(c.user)}</td><td class="mono trunc" title="${esc(c.command)}">${esc(c.command)}</td><td class="muted trunc-s">${esc(c.source)}</td></tr>`).join('')}</tbody></table></div>` : '<p class="empty">Nenhuma entrada de cron.</p>')
        + (per ? `<div class="toolbar" style="margin-top:10px">${per}</div>` : '') + '<p class="muted">Variáveis de ambiente do crontab não são exibidas e valores que parecem senhas/tokens são mascarados.</p>');
    } catch (e) { html('sv-cron', `<p class="empty">Erro: ${esc(e.message)}</p>`); }
  }
  async function loadContainers(stats) {
    try {
      const r = await api('/api/containers' + (stats ? '?stats=1' : ''));
      if (!r.available) { html('sv-ct', `<p class="empty">Docker indisponível: ${esc(r.error || '')}</p>`); return; }
      const st = (c) => (c.state === 'running' ? pill('rodando', 'ok') : c.state === 'exited' ? pill('parado') : c.state === 'paused' ? pill('pausado', 'warn') : c.state === 'restarting' ? pill('reiniciando', 'info') : pill(c.state, c.state === 'dead' ? 'crit' : ''));
      html('sv-ct', r.containers.length ? `<p class="sub">Docker ${esc(r.version)} · ${r.containers.filter((c) => c.state === 'running').length} de ${r.containers.length} em execução</p><div class="table-wrap"><table class="t-compact"><thead><tr><th>Contêiner</th><th>Imagem</th><th>Estado</th><th>Status</th><th>Portas</th><th class="num">CPU</th><th class="num">Memória</th><th class="num">Ações</th></tr></thead><tbody>
        ${r.containers.map((c) => `<tr><td class="trunc-s" title="${esc(c.name)}"><b>${esc(c.name)}</b><div class="muted mono" style="font-size:11.5px">${esc(c.id)}${c.compose ? ' · ' + esc(c.compose) : ''}</div></td><td class="trunc-s">${esc(c.image)}</td><td>${st(c)}</td><td class="trunc-s">${esc(c.status)}</td><td class="mono trunc-s" title="${esc(c.ports)}">${esc(c.ports || '—')}</td>
          <td class="num">${c.cpu == null ? '—' : pct(c.cpu)}</td><td class="num">${esc(c.mem || '—')}</td>
          <td><div class="actions" data-ct="${esc(c.id)}" data-ctname="${esc(c.name)}">${c.state === 'running' ? '<button class="btn sm" data-c="restart">Reiniciar</button><button class="btn sm danger" data-c="stop">Parar</button>' : '<button class="btn sm primary" data-c="start">Iniciar</button>'}</div></td></tr>`).join('')}
        </tbody></table></div>` : '<p class="empty">Nenhum contêiner.</p>');
    } catch (e) { html('sv-ct', `<p class="empty">Erro: ${esc(e.message)}</p>`); }
  }
  page('servicos', 'Serviços', 'gear', {
    mount(el) {
      el.innerHTML = panel('', `<div class="toolbar" style="margin-bottom:12px">
          <span class="seg" id="sv-scope"><button data-s="user" aria-pressed="${SV.scope === 'user'}">Usuário</button><button data-s="system" aria-pressed="${SV.scope === 'system'}">Sistema</button></span>
          <span class="seg" id="sv-filter">${[['todos', 'Todos'], ['ativos', 'Ativos'], ['falhas', 'Falhas'], ['inativos', 'Inativos']].map(([f, l]) => `<button data-f="${f}" data-label="${l}" aria-pressed="${SV.filter === f}">${l}</button>`).join('')}</span>
          <input id="sv-q" class="input" type="search" placeholder="Buscar serviço" value="${esc(SV.q)}">
          <span class="sp"></span><button class="btn sm" id="sv-reload">Atualizar</button></div>
        <p class="sub" id="sv-note"></p>
        <div class="table-wrap tall" id="sv-table"><p class="empty">Carregando…</p></div>`)
        + panel('Timers do systemd', '<div id="sv-timers"><p class="empty">Carregando…</p></div>', '<span class="sp"></span><span class="muted" id="sv-timers-scope" style="text-transform:none;letter-spacing:0;font-weight:500"></span>')
        + panel('Cron', '<div id="sv-cron"><p class="empty">Carregando…</p></div>')
        + panel('Contêineres (Docker)', '<div id="sv-ct"><p class="empty">Carregando…</p></div>', '<span class="sp"></span><button class="btn sm" id="sv-ct-stats">Medir CPU/memória</button><button class="btn sm" id="sv-ct-reload">Atualizar</button>');
      const note = () => { $('#sv-timers-scope').textContent = SV.scope === 'user' ? 'escopo: usuário' : 'escopo: sistema'; loadTimers();  $('#sv-note').innerHTML = SV.scope === 'system' ? 'Ações no escopo do <b>sistema</b> pedem autorização ao polkit (sem sudo). <b>Este fluxo não foi testado</b>; se não houver agente polkit na sessão, a ação falha.' : 'Serviços do seu usuário (<code class="cmd">systemctl --user</code>).'; };
      note();
      $('#sv-scope').addEventListener('click', (e) => { const b = e.target.closest('button'); if (!b) return; SV.scope = b.dataset.s; $$('#sv-scope button').forEach((x) => x.setAttribute('aria-pressed', String(x === b))); note(); html('sv-table', '<p class="empty">Carregando…</p>'); loadServices(); });
      $('#sv-filter').addEventListener('click', (e) => { const b = e.target.closest('button'); if (!b) return; SV.filter = b.dataset.f; $$('#sv-filter button').forEach((x) => x.setAttribute('aria-pressed', String(x === b))); renderServices(); });
      $('#sv-q').addEventListener('input', (e) => { SV.q = e.target.value; renderServices(); });
      $('#sv-reload').addEventListener('click', loadServices);
      $('#sv-table').addEventListener('click', async (e) => {
        const b = e.target.closest('button[data-v]');
        if (!b) return;
        const unit = b.parentElement.dataset.unit, v = b.dataset.v;
        const verb = { start: 'Iniciar', stop: 'Parar', restart: 'Reiniciar' }[v];
        if (!(await confirmAction(`${verb} serviço?`, `<p>${verb} <b>${esc(unit)}</b> (escopo ${SV.scope === 'user' ? 'usuário' : 'sistema'})?</p><p class="muted">A ação é registrada no log de auditoria.</p>`, verb, v === 'stop' ? 'danger' : 'primary'))) return;
        try { await api('/api/services/action', { method: 'POST', body: { scope: SV.scope, unit, action: v } }); toast(`${verb}: ${unit} — ok`); } catch (err) { toast('Falhou: ' + err.message, true); }
        loadServices();
      });
      $('#sv-ct-reload').addEventListener('click', () => loadContainers(false));
      $('#sv-ct-stats').addEventListener('click', () => { html('sv-ct', '<p class="empty">Medindo (docker stats leva alguns segundos)…</p>'); loadContainers(true); });
      $('#sv-ct').addEventListener('click', async (e) => {
        const b = e.target.closest('button[data-c]');
        if (!b) return;
        const box = b.parentElement, v = b.dataset.c;
        const verb = { start: 'Iniciar', stop: 'Parar', restart: 'Reiniciar' }[v];
        if (!(await confirmAction(`${verb} contêiner?`, `<p>${verb} <b>${esc(box.dataset.ctname)}</b>?</p><p class="muted">A ação é registrada no log de auditoria.</p>`, verb, v === 'stop' ? 'danger' : 'primary'))) return;
        try { await api('/api/containers/action', { method: 'POST', body: { id: box.dataset.ct, action: v } }); toast(`${verb}: ${box.dataset.ctname} — ok`); } catch (err) { toast('Falhou: ' + err.message, true); }
        loadContainers(false);
      });
      loadServices();
      loadCron();
      loadContainers(false);
    },
    unmount() { clearTimeout(SV.timer); },
  });

  // ---- 7. Rede
  page('rede', 'Rede', 'globe', {
    mount(el) {
      el.innerHTML = `
        <div class="grid g3">${kpi('n-rx')}${kpi('n-tx')}${kpi('n-tcp')}</div>
        ${card('Tráfego', '<div id="n-chart"></div><div class="series-legend"><span><i class="dot" style="background:var(--acento)"></i>Download</span><span><i class="dot" style="background:var(--violeta)"></i>Upload</span></div>')}
        ${card('Interfaces', '<div id="n-if"></div>')}
        ${card('Portas em escuta', '<div id="n-listen"></div>', '<span class="sp"></span><span class="muted" style="text-transform:none;letter-spacing:0;font-weight:500">processos de outros usuários aparecem sem dono</span>')}`;
    },
    update(s) {
      const n = s.net;
      setKpi('n-rx', 'Download', '↓ ' + rate(n.rx_bps), 'todas as interfaces físicas');
      setKpi('n-tx', 'Upload', '↑ ' + rate(n.tx_bps), 'todas as interfaces físicas');
      const tcp = n.tcp || {};
      const others = Object.entries(tcp).filter(([k]) => k !== 'ESTABLISHED' && k !== 'LISTEN').map(([k, v]) => `${k.toLowerCase().replace('_', ' ')} ${v}`).join(' · ');
      setKpi('n-tcp', 'Conexões TCP', num(tcp.ESTABLISHED || 0), `estabelecidas · escutando ${tcp.LISTEN || 0}${others ? ' · ' + others : ''}`);
      chart($('#n-chart'), { series: [{ data: S.hist.rx, color: 'var(--acento)' }, { data: S.hist.tx, color: 'var(--violeta)' }], ts: S.hist.ts, max: 'auto', fmt: (v) => rate(v) });
      html('n-if', `<div class="table-wrap"><table><thead><tr><th>Interface</th><th>Estado</th><th class="num">Velocidade</th><th class="num">↓ agora</th><th class="num">↑ agora</th><th class="num">Recebido</th><th class="num">Enviado</th><th class="num">Erros/descartes</th></tr></thead><tbody>
        ${n.ifaces.map((i) => `<tr><td><b>${esc(i.name)}</b> ${i.virtual ? '<span class="muted">virtual</span>' : ''}<div class="muted mono" style="font-size:11.5px">${esc(i.mac)}</div></td><td>${i.state === 'up' ? pill('ativa', 'ok') : i.state === 'unknown' ? pill('desconhecido') : pill(i.state === 'down' ? 'inativa' : i.state, i.state === 'down' ? 'warn' : '')}</td>
          <td class="num">${i.speed_mbps ? num(i.speed_mbps) + ' Mb/s' : '—'}</td><td class="num">${rate(i.rx_bps)}</td><td class="num">${rate(i.tx_bps)}</td><td class="num">${bytes(i.rx_bytes)}</td><td class="num">${bytes(i.tx_bytes)}</td><td class="num">${num(i.rx_errs + i.tx_errs)}</td></tr>`).join('')}
        </tbody></table></div>`);
      html('n-listen', n.listening.length ? `<div class="table-wrap tall"><table><thead><tr><th>Proto</th><th>Endereço</th><th class="num">Porta</th><th>Processo</th><th class="num">PID</th><th>Alcance</th></tr></thead><tbody>
        ${n.listening.map((l) => `<tr><td>${esc(l.proto)}</td><td class="mono">${esc(l.addr)}</td><td class="num"><b>${l.port}</b></td><td>${esc(l.process || '—')}</td><td class="num">${l.pid ?? '—'}</td><td>${l.scope === 'local' ? pill('só local', 'ok') : pill('acessível na rede', 'warn')}</td></tr>`).join('')}
        </tbody></table></div>` : '<p class="empty">Nenhuma porta em escuta.</p>');
    },
  });

  // ---- 8. Logs
  const LG = { scope: store.get('pulse.logscope') || 'system', unit: '', prio: '', lines: '200', q: '', follow: false, cursor: '', timer: 0, rows: [] };
  const PRIO = ['emergência', 'alerta', 'crítico', 'erro', 'aviso', 'notícia', 'info', 'debug'];
  function logRow(l) {
    const d = new Date(l.ts);
    const when = d.toLocaleDateString('pt-BR', { day: '2-digit', month: '2-digit' }) + ' ' + d.toLocaleTimeString('pt-BR');
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
        box.innerHTML = r.lines.length ? r.lines.map(logRow).join('') : `<p class="empty">${r.note ? esc(r.note) : 'Nenhuma linha.'}</p>`;
        box.scrollTop = box.scrollHeight;
      }
      $('#lg-count').textContent = `${box.querySelectorAll('.log').length} linhas`;
    } catch (e) { html('lg-box', `<p class="empty">Erro: ${esc(e.message)}</p>`); }
    if (LG.follow && S.page && S.page.id === 'logs') LG.timer = setTimeout(() => loadLogs(true), 2000);
  }
  page('logs', 'Logs', 'doc', {
    mount(el) {
      el.innerHTML = panel('', `<div class="toolbar" style="margin-bottom:12px">
          <span class="seg" id="lg-scope"><button data-s="system" aria-pressed="${LG.scope === 'system'}">Sistema</button><button data-s="user" aria-pressed="${LG.scope === 'user'}">Usuário</button></span>
          <input id="lg-unit" class="input" placeholder="unidade (ex.: ssh.service)" value="${esc(LG.unit)}" style="width:200px">
          <select id="lg-prio" class="input" aria-label="Prioridade"><option value="">todas as prioridades</option>${PRIO.map((n, i) => `<option value="${i}" ${String(i) === LG.prio ? 'selected' : ''}>até ${i} · ${n}</option>`).join('')}</select>
          <select id="lg-lines" class="input" aria-label="Linhas">${['100', '200', '500', '1000'].map((n) => `<option ${n === LG.lines ? 'selected' : ''}>${n}</option>`).join('')}</select>
          <input id="lg-q" class="input" type="search" placeholder="Buscar texto" value="${esc(LG.q)}" style="flex:1;min-width:160px">
          <label class="chk"><input type="checkbox" id="lg-follow" ${LG.follow ? 'checked' : ''}> seguir</label>
          <button class="btn sm primary" id="lg-go">Atualizar</button></div>
        <div class="toolbar" style="margin-bottom:8px"><span class="pill crit">erro+</span><span class="pill warn">aviso</span><span class="pill info">notícia</span><span class="pill">info</span><span class="sp"></span><span class="muted" id="lg-count"></span></div>
        <div class="logs" id="lg-box"><p class="empty">Carregando…</p></div>`);
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
  function renderAgentsPage() {
    const A = S.agents;
    if (!A || !$('#ag-kpis')) return;
    if (!A.enabled) { html('ag-body', '<p class="empty">Detecção de agentes desativada (--no-agents).</p>'); return; }
    const ps = A.processes, ss = A.sessions;
    const mcp = ps.flatMap((p) => p.mcp);
    const day = ss.filter((s) => Date.now() - s.updated < 86400e3);
    const active = ss.filter((s) => Date.now() - s.updated < 5 * 60e3).length;
    const tin = day.reduce((a, s) => a + s.tokens_in + s.tokens_cache, 0), tout = day.reduce((a, s) => a + s.tokens_out, 0);
    setKpi('ag-k1', 'Processos de agentes', String(ps.length), [...new Set(ps.map((p) => p.agent))].join(', ') || 'nenhum');
    setKpi('ag-k2', 'Servidores MCP', String(mcp.length), [...new Set(mcp.map((m) => m.name))].join(', ') || 'nenhum detectado');
    setKpi('ag-k3', 'Sessões ativas / recentes', `${active} / ${day.length}`, 'ativas: últimos 5 min · recentes: 24 h');
    setKpi('ag-k4', 'Tokens (sessões lidas)', kilo(tin + tout), `${kilo(tin)} entrada (c/ cache) · ${kilo(tout)} saída`);
    const hs = A.hooks.sessions;
    html('ag-live', hs.length ? `<div class="grid g3">${hs.slice(0, 6).map((h) => `<div class="agent-card"><div class="hd"><strong>${esc(agentName(h.agent))}</strong><span class="sp"></span>${statePill(h.state)}</div>
        <div class="meta">sessão ${esc(h.session.slice(0, 8))} · ${ago(h.ts)}${h.cwd ? ' · ' + esc(h.cwd.split('/').pop()) : ''}</div>
        <div class="toolbar">${h.event ? pill(h.event, 'info') : ''}<span class="sub trunc-s">${esc(h.tool || h.message || '')}</span></div></div>`).join('')}</div>`
      : '<p class="empty">Nenhum evento de hook recebido ainda. Use “Conectar hooks” para o Claude Code enviar estados ao vivo.</p>');
    html('ag-procs', ps.length ? `<div class="grid g2">${ps.map((p) => `<div class="agent-card"><div class="hd"><strong>${esc(p.agent)}</strong><span class="muted">${esc(p.name)}</span><span class="sp"></span>${p.state === 'T' ? pill('pausado', 'warn') : pill('ativo', 'ok')}</div>
        <div class="meta"><span>PID <b>${p.pid}</b></span><span>CPU <b>${pct(p.tree_cpu)}</b></span><span>RAM <b>${bytes(p.tree_rss)}</b></span><span>${p.children} filho${p.children === 1 ? '' : 's'}</span></div>
        <div class="meta mono trunc" title="${esc(p.cwd || '')}">${esc(p.cwd || '—')}</div>
        ${p.mcp.length ? `<div class="meta">MCP: ${p.mcp.map((m) => pill(m.name, 'vio')).join(' ')}</div>` : ''}
        <div class="actions" data-pid="${p.pid}" data-name="${esc(p.agent)}" style="justify-content:flex-start;margin-top:8px">
          <button class="btn sm" data-a="${p.state === 'T' ? 'resume' : 'pause'}">${p.state === 'T' ? 'Continuar' : 'Pausar'}</button>
          <button class="btn sm danger" data-a="terminate">Encerrar</button></div></div>`).join('')}</div>`
      : '<p class="empty">Nenhum processo de agente em execução.</p>');
    const hookBySession = Object.fromEntries(hs.map((h) => [h.session, h]));
    html('ag-sessions', ss.length ? `<div class="table-wrap"><table><thead><tr><th>Agente</th><th>Projeto</th><th>Estado</th><th>Modelo</th><th class="num">Pedidos</th><th class="num">Ferramentas</th><th class="num">Tokens in/out</th><th class="num">Atividade</th></tr></thead><tbody>
      ${ss.map((s) => { const h = hookBySession[s.id]; const tools = s.tools.reduce((a, t) => a + t.n, 0); return `<tr class="clickable" data-sid="${esc(s.id)}" tabindex="0" aria-expanded="${openSession === s.id}">
        <td><b>${esc(s.agent)}</b></td><td><div class="trunc-s"><b>${esc(s.project_name || '—')}</b></div><div class="muted mono trunc-s" style="font-size:11.5px">${esc(s.project)}</div></td>
        <td>${h ? statePill(h.state) : Date.now() - s.updated < 120e3 ? pill('ativa', 'ok') : pill('inativa')}</td><td class="trunc-s">${esc(s.model || '—')}</td>
        <td class="num">${s.prompts}</td><td class="num">${tools}</td><td class="num">${kilo(s.tokens_in + s.tokens_cache)} / ${kilo(s.tokens_out)}</td><td class="num">${ago(s.updated)}</td></tr>`; }).join('')}
      </tbody></table></div><div id="ag-timeline"></div>` : '<p class="empty">Nenhuma transcrição recente em ~/.claude/projects ou ~/.codex/sessions.</p>');
    renderTimeline();
    const ev = A.hooks.events;
    html('ag-events', ev.length ? ev.slice(0, 30).map((e) => `<div class="toolbar" style="margin-bottom:6px"><span class="muted" style="width:70px">${ago(e.ts)}</span><b>${esc(agentName(e.agent))}</b>${statePill(e.state)}${e.event ? pill(e.event, 'info') : ''}<span class="sub trunc">${esc(e.tool || e.message)}</span></div>`).join('') : '<p class="empty">Sem eventos.</p>');
  }
  function renderTimeline() {
    const el = $('#ag-timeline');
    if (!el) return;
    const s = S.agents && S.agents.sessions.find((x) => x.id === openSession);
    if (!s) { el.innerHTML = ''; return; }
    const kinds = { prompt: ['var(--texto-3)', 'Pedido'], tool: ['var(--acento)', ''], reply: ['var(--violeta)', 'Resposta'], done: ['var(--ok)', 'Concluído'] };
    el.innerHTML = `<div class="toolbar" style="margin-top:12px"><b>Linha do tempo</b><span class="muted">${esc(s.title || s.id)}</span><span class="sp"></span>${s.tools.slice(0, 8).map((t) => pill(`${t.name} ×${t.n}`, 'info')).join(' ')}</div>
      <ol class="timeline">${s.timeline.slice().reverse().map((t) => { const [c, l] = kinds[t.kind] || ['var(--texto-3)', t.kind]; return `<li><i class="dot" style="background:${c}"></i><time>${esc(clock(t.ts))}</time><span>${esc(l || 'Ferramenta')}${t.kind === 'tool' ? ' · <b>' + esc(t.label) + '</b>' : ''}</span></li>`; }).join('')}</ol>
      ${s.partial ? '<p class="muted">Transcrição grande: só o trecho final foi lido.</p>' : ''}<p class="muted">Por privacidade, o Pulse mostra só nomes de ferramentas — nunca o texto dos pedidos ou respostas.</p>`;
  }
  async function hooksDialog() {
    let info;
    try { info = await api('/api/agents/hooks'); } catch (e) { toast(e.message, true); return; }
    const conn = info.connected.length;
    const ok = await dialog({
      title: 'Conectar hooks',
      body: `<p>Os hooks fazem o Claude Code avisar o Pulse quando está <b>executando</b>, <b>pedindo atenção</b>, <b>aguardando</b> ou <b>concluído</b>.</p>
        <dl class="kv"><dt>Arquivo</dt><dd class="mono trunc-s" title="${esc(info.settings)}">${esc(info.settings)}</dd>
        <dt>Script</dt><dd class="mono trunc-s" title="${esc(info.script)}">${esc(info.script)}</dd>
        <dt>Script instalado</dt><dd>${info.script_exists ? pill('sim', 'ok') : pill('não — rode install.sh', 'warn')}</dd>
        <dt>Eventos conectados</dt><dd>${conn} de ${info.total_events}</dd></dl>
        <p class="sub">Instalar adiciona estes hooks ao <code class="cmd">settings.json</code>, preservando o que já existe e criando um backup <code class="cmd">settings.json.pulse-bak</code>:</p>
        <pre class="snippet mono">${esc(info.snippet)}</pre>
        <p class="sub"><b>Codex</b> ${info.codex_connected ? pill('conectado', 'ok') : pill('não conectado')} — adicione no início de <code class="cmd">~/.codex/config.toml</code> (antes de qualquer <code class="cmd">[tabela]</code>):</p>
        <pre class="snippet mono">${esc(info.codex_snippet)}</pre>
        <p class="muted">O Pulse não edita o config.toml do Codex automaticamente. OpenClaw: chame <code class="cmd">pulse-hook &lt;estado&gt; openclaw</code> a partir das notificações dele.</p>`,
      actions: [{ label: 'Fechar', value: false }, { label: conn === info.total_events ? 'Já conectado' : 'Instalar no Claude Code', cls: 'primary', value: conn !== info.total_events }],
    });
    if (!ok) return;
    try {
      const r = await api('/api/agents/hooks/install', { method: 'POST', body: {} });
      toast(r.added.length ? `Hooks adicionados: ${r.added.join(', ')}${r.backup ? ' · backup em ' + r.backup : ''}` : 'Os hooks já estavam conectados.');
    } catch (e) { toast('Falhou: ' + e.message, true); }
  }
  page('agentes', 'Agentes', 'bot', {
    mount(el) {
      el.innerHTML = `
        <div class="toolbar"><p class="lead">Processos, sessões e eventos dos agentes de IA rodando nesta máquina.</p><span class="sp"></span><button class="btn" id="ag-hooks">Conectar hooks</button></div>
        <div id="ag-body" style="display:contents">
        <div class="grid g4" id="ag-kpis">${kpi('ag-k1')}${kpi('ag-k2')}${kpi('ag-k3')}${kpi('ag-k4')}</div>
        ${panel('Ao vivo (hooks)', '<div id="ag-live"></div>')}
        ${panel('Processos', '<div id="ag-procs"></div>')}
        ${panel('Sessões (transcrições)', '<div id="ag-sessions"></div>')}
        ${panel('Eventos recentes (hooks)', '<div id="ag-events"></div>')}</div>`;
      $('#ag-hooks').addEventListener('click', hooksDialog);
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
      html('al-active', r.active.length ? r.active.map((a) => `<div class="toolbar" style="margin-bottom:10px">${sevPill(a.severity)}<b>${esc(a.title)}</b><span class="sub">${esc(a.detail)}</span><span class="sp"></span><span class="muted">desde ${esc(clock(a.since))} (${ago(a.since)})</span></div>`).join('') : '<p class="empty">Tudo certo — nenhum alerta ativo.</p>');
      html('al-rules', `<table><thead><tr><th>Regra</th><th>Condição</th><th>Severidade</th></tr></thead><tbody>${r.rules.map((x) => `<tr><td><b>${esc(x.title)}</b></td><td>${esc(x.condition)}</td><td>${sevPill(x.severity)}</td></tr>`).join('')}</tbody></table>`);
      html('al-hist', r.history.length ? `<div class="table-wrap tall"><table><thead><tr><th>Quando</th><th>Evento</th><th>Alerta</th><th>Detalhe</th></tr></thead><tbody>${r.history.map((h) => `<tr><td class="num">${esc(dateTime(h.ts))}</td><td>${h.kind === 'resolvido' ? pill('resolvido', 'ok') : h.kind === 'agravou' ? pill('agravou', 'crit') : pill('disparou', 'warn')}</td><td><b>${esc(h.alert.title)}</b></td><td>${esc(h.alert.detail)}</td></tr>`).join('')}</tbody></table></div>` : '<p class="empty">Nenhum evento desde que o Pulse iniciou.</p>');
      html('al-notify', r.notify ? `${pill('ativada', 'ok')} <span class="sub">Alertas novos vão para o desktop via <code class="cmd">notify-send</code>.</span>` : `${pill('desativada')} <span class="sub">Inicie o Pulse com <code class="cmd">--notify</code> para receber notificações no desktop.</span>`);
    } catch { /* tenta de novo */ }
    if (S.page && S.page.id === 'alertas') alertsTimer = setTimeout(loadAlerts, 3000);
  }
  page('alertas', 'Alertas', 'bell', {
    mount(el) {
      el.innerHTML = `${panel('Ativos agora', '<div id="al-active"></div>')}
        <div class="grid g-main">${card('Histórico', '<div id="al-hist"></div>')}<div class="grid" style="align-content:start">${card('Regras', '<div class="table-wrap" id="al-rules"></div>')}${card('Notificação no desktop', '<div class="toolbar" id="al-notify"></div>')}</div></div>`;
      loadAlerts();
    },
    unmount() { clearTimeout(alertsTimer); },
  });

  // ------------------------------------------------------------ cabeçalho e rotas
  function renderNav() {
    $('#nav').innerHTML = PAGES.map((p) => `<a class="nav-item" href="#/${p.id}" data-id="${p.id}">${ICONS[p.icon]}<span>${esc(p.title)}</span>${p.id === 'alertas' ? '<span class="badge" id="nav-badge" hidden></span>' : ''}</a>`).join('');
  }
  function renderHeader(s) {
    $('#page-sub').textContent = `${s.host.os} · kernel ${s.host.kernel} · ligado há ${dur(s.host.uptime_s)}`;
    $('#brand-host').textContent = s.host.hostname;
    S.user = s.host.user;
    $('#chip-self').textContent = `Pulse ${pct(s.pulse.cpu)} CPU · ${bytes(s.pulse.rss)}`;
    const n = s.alerts.length;
    const crit = s.alerts.some((a) => a.severity === 'critico');
    const ch = $('#chip-alerts');
    ch.textContent = n === 1 ? '1 alerta' : `${n} alertas`;
    ch.className = 'chip' + (n ? (crit ? ' crit' : ' warn') : '');
    const b = $('#nav-badge');
    if (b) { b.hidden = !n; b.textContent = n; b.className = 'badge' + (crit ? ' crit' : ''); }
    document.title = n ? `(${n}) Pulse` : 'Pulse';
  }
  function route() {
    const id = (location.hash.match(/^#\/([\w-]+)/) || [])[1] || 'visao-geral';
    const p = PAGES.find((x) => x.id === id) || PAGES[0];
    if (S.page && S.page.unmount) S.page.unmount();
    S.page = p;
    $$('.nav-item').forEach((a) => (a.dataset.id === p.id ? a.setAttribute('aria-current', 'page') : a.removeAttribute('aria-current')));
    $('#page-title').textContent = p.title;
    const el = $('#page');
    el.innerHTML = '';
    p.mount(el);
    if (S.snap && p.update) p.update(S.snap);
    el.focus({ preventScroll: true });
    window.scrollTo(0, 0);
  }
  window.addEventListener('hashchange', route);
  let resizeT = 0;
  window.addEventListener('resize', () => { clearTimeout(resizeT); resizeT = setTimeout(() => { if (S.snap && S.page && S.page.update) S.page.update(S.snap); }, 150); });

  let raf = 0;
  function onSnapshot(s) {
    S.snap = s;
    pushPoint(s);
    if (raf) return;
    raf = requestAnimationFrame(() => {
      raf = 0;
      renderHeader(S.snap);
      if (S.page && S.page.update) S.page.update(S.snap);
    });
  }

  function startStream() {
    if (S.es) S.es.close();
    const es = new EventSource('/api/stream');
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
  // Refração SVG só no Chromium; os demais ficam com o blur do CSS.
  try {
    const brands = (navigator.userAgentData && navigator.userAgentData.brands) || [];
    if (brands.some((b) => /Chromium/i.test(b.brand))) document.documentElement.classList.add('refract');
  } catch { /* fallback de blur */ }

  // ------------------------------------------------------------ início
  async function boot() {
    renderNav();
    const m = location.hash.match(/^#token=([0-9a-fA-F]{32,})/);
    if (m) {
      history.replaceState(null, '', location.pathname + '#/visao-geral');
      try { await login(m[1]); } catch { toast('Token da URL inválido.', true); }
    }
    let h = null;
    try { h = await (await fetch('/api/health', { credentials: 'same-origin' })).json(); } catch { /* fora do ar */ }
    if (!h) { $('#page-sub').textContent = 'Daemon indisponível.'; return; }
    if (!h.authenticated) {
      if (token()) { try { await login(token()); } catch { session.del('pulse.token'); } }
      h = await (await fetch('/api/health', { credentials: 'same-origin' })).json();
      if (!h.authenticated) { route(); await showLogin(); return; }
    }
    try {
      const hist = await api('/api/history');
      for (const k in S.hist) S.hist[k] = (hist[k] || []).map((v) => (v == null ? null : v));
    } catch { /* começa vazio */ }
    try { const snap = await api('/api/snapshot'); if (snap && snap.cpu) { S.snap = snap; S.lastSeq = snap.seq; renderHeader(snap); } } catch { /* idem */ }
    route();
    startStream();
  }
  boot();
})();
