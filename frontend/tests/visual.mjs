import assert from 'node:assert/strict'
import { createServer } from 'node:http'
import { mkdir, readFile, writeFile } from 'node:fs/promises'
import { extname, resolve } from 'node:path'
import { chromium } from 'playwright'

// Check rendered geometry against isolated fixtures, including awkward widths,
// long resource names, large counters, errors, empty states and short screens.
const dist = resolve(import.meta.dirname, '../dist')
const output = resolve(import.meta.dirname, '../../artifacts/visual', new Date().toISOString().replace(/[:.]/g, '-'))
await mkdir(output, { recursive: true })
const server = createServer(async (req, res) => {
  try {
    const path = new URL(req.url, 'http://localhost').pathname
    const file = path.startsWith('/assets/') ? resolve(dist, `.${path}`) : resolve(dist, 'index.html')
    if (!file.startsWith(dist + '/')) return res.writeHead(403).end()
    res.setHeader('Content-Type', ({ '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css' })[extname(file)] ?? 'application/octet-stream')
    res.end(await readFile(file))
  } catch { res.writeHead(404).end() }
})
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve))
const url = `http://127.0.0.1:${server.address().port}`
const failures = [], captures = [], errors = []
let browser
try {
  browser = await chromium.launch({ channel: 'chrome', headless: true })
  const page = await browser.newPage({ reducedMotion: 'reduce' })
  page.on('pageerror', error => errors.push(error.message))
  page.on('dialog', dialog => dialog.accept())
  let configured = true, empty = false, stress = false, fail = false, failConfig = false, configGate = null
  const longName = 'Engineering-production-infrastructure-and-observability-'.repeat(2)
  const groups = () => empty ? [] : [
    { id: 'engineering', name: stress ? longName : 'Engineering', allowed_groups: ['services'] },
    { id: 'operations', name: 'Operations', allowed_groups: [] },
    { id: 'services', name: 'Services', allowed_groups: [] },
  ]
  const peers = () => empty ? [] : [
    { id: 'mac', name: stress ? longName : 'MacBook Pro', group_id: 'engineering', ipv4: '192.168.100.254', public_key: 'fixture', sent_bytes: stress ? 999_999_999_999 : 1_820_000, received_bytes: stress ? 999_999_999_999 : 4_600_000, last_handshake_unix: Math.floor(Date.now() / 1000) },
    { id: 'server', name: 'Build server', group_id: 'services', ipv4: '192.168.100.253', public_key: 'fixture', sent_bytes: 65_400_000, received_bytes: 87_600_000, last_handshake_unix: null },
    { id: 'idle', name: 'Idle peer', group_id: 'engineering', ipv4: '192.168.100.252', public_key: 'fixture', sent_bytes: 0, received_bytes: 0, last_handshake_unix: null },
  ]
  const forwards = () => empty ? [] : [
    { id: 'docs', name: stress ? longName : 'Internal docs', protocol: 'tcp', listen_port: 65535, target_port: 65535, target_peer_id: 'mac', allowed_group_ids: stress ? ['engineering', 'operations', 'services'] : ['engineering'] },
    { id: 'build', name: 'Build cache', protocol: 'udp', listen_port: 8080, target_port: 80, target_peer_id: 'server', allowed_group_ids: [] },
  ]
  const settings = () => ({ subnet: '192.168.100.0/24', endpoint: stress ? `${'edge-'.repeat(10)}gateway.example.com:65535` : 'vpn.example.com:51820', persistent_keepalive: 25 })
  await page.route('**/api/**', async route => {
    const request = route.request(), path = new URL(request.url()).pathname, body = request.postDataJSON()
    const reply = (json, status = 200) => route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(json) })
    if (request.headers().authorization !== 'Bearer visual-fixture-token') return reply({ message: 'Invalid access token.' }, 401)
    if (path === '/api/setup') return reply({ configured, settings: configured ? settings() : null })
    if (path === '/api/status') return fail ? reply({ message: 'Runtime status unavailable. Retry when the service is ready.' }, 503) : reply({ ready: true, persisted_revision: 1, applied_revision: 1, last_activation_error: null })
    if (path === '/api/config') {
      if (configGate) { const gate = configGate; configGate = null; gate.seen(); await gate.released }
      return failConfig ? reply({ message: 'Unable to load forward configuration. Retry to read the latest settings.' }, 503) : reply({ revision: 1, groups: groups(), peers: peers(), forwards: forwards(), settings: settings() })
    }
    if (path === '/api/policy') return reply({ code: 'revision_conflict', message: 'Configuration changed', persisted_revision: 2 }, 409)
    if (path === '/api/peers' && request.method() === 'POST') return reply({ peer: { ...peers()[0], name: body.name }, config: '[Interface]\nPrivateKey = fixture-only\nAddress = 192.168.100.254/32\n\n[Peer]\nPublicKey = fixture\nEndpoint = vpn.example.com:51820' })
    if (path === '/api/peers') return reply(peers())
    if (path === '/api/forwards') return fail ? reply({ message: 'Forward configuration is temporarily unavailable. Please retry.' }, 503) : reply(forwards())
    return reply({}, 404)
  })
  const navigate = async name => {
    const open = page.getByRole('button', { name: 'Open navigation', exact: true })
    if (await open.isVisible()) await open.click()
    await page.getByRole('navigation').getByRole('button', { name, exact: ['Overview', 'Groups', 'Settings'].includes(name) }).click()
    await page.getByRole('heading', { name, exact: true, level: 1 }).waitFor()
    await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))))
  }
  const login = async () => {
    await page.getByLabel('Access token').fill('visual-fixture-token')
    await page.getByRole('button', { name: 'Connect', exact: true }).click()
    await page.getByRole('heading', { name: configured ? 'Overview' : 'Create your network.', exact: true }).waitFor()
    if (configured) await page.waitForFunction(() => !document.querySelector('[aria-label="Refresh data"]').disabled)
  }
  const capture = async name => {
    await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))))
    const findings = await page.evaluate(() => {
      const issues = []
      const rect = element => element.getBoundingClientRect()
      const visible = element => rect(element).width > 0 && rect(element).height > 0 && getComputedStyle(element).visibility !== 'hidden'
      const close = (a, b) => Math.abs(a - b) <= 1
      const label = element => `${element.tagName.toLowerCase()}.${element.className?.baseVal ?? element.className}`
      const active = document.querySelector('.dialog') ?? document.querySelector('.page-enter') ?? document.querySelector('.auth-card')
      if (document.documentElement.scrollWidth > innerWidth) issues.push(`Page overflows by ${document.documentElement.scrollWidth - innerWidth}px`)
      for (const element of [...active.querySelectorAll('*'), ...document.querySelectorAll('.topbar, .topbar *, .runtime-strip, .runtime-strip *, .sidebar-open, .sidebar-open *')]) {
        if (!visible(element) || element.closest('.react-flow, .config-details pre') || element.tagName === 'OPTION') continue
        const box = rect(element), style = getComputedStyle(element)
        if (box.left < -1 || box.right > innerWidth + 1) issues.push(`${label(element)} extends outside viewport (${box.left.toFixed(1)}..${box.right.toFixed(1)})`)
        if (element.children.length === 0 && element.textContent.trim() && element.scrollWidth > element.clientWidth + 1 && style.overflowX === 'visible' && style.display !== 'inline') issues.push(`${label(element)} text overflows its box by ${element.scrollWidth - element.clientWidth}px`)
      }
      for (const selector of ['.panel-head', '.dialog-head', '.button', '.icon-button']) {
        for (const element of active.querySelectorAll(selector)) {
          if (!visible(element)) continue
          const box = rect(element)
          for (const child of element.children) {
            if (!visible(child)) continue
            const c = rect(child)
            if (c.left < box.left - 1 || c.right > box.right + 1) issues.push(`${selector} child extends beyond container`)
            if (!close(c.top + c.height / 2, box.top + box.height / 2)) issues.push(`${selector} child is vertically off-center by ${Math.abs(c.top + c.height / 2 - box.top - box.height / 2).toFixed(1)}px`)
          }
        }
      }
      const title = active.querySelector('.page-heading h1'), action = active.querySelector('.page-heading > button, .page-heading > .update-time')
      if (title && action && !close(rect(title).top + rect(title).height / 2, rect(action).top + rect(action).height / 2)) issues.push('Page title and its action are not vertically aligned')
      const cards = [...active.querySelectorAll('.metric-card')]
      for (const card of cards) {
        const value = card.querySelector('.metric-value'), box = rect(value)
        if (value.scrollWidth > value.clientWidth + 1) issues.push('Metric value overflows its card')
        for (const other of cards.filter(other => close(rect(other).top, rect(card).top))) {
          if (!close(box.top, rect(other.querySelector('.metric-value')).top)) issues.push('Metric values in the same row do not share a baseline')
        }
      }
      const overviewHeads = [...active.querySelectorAll('.overview-grid .panel-head h2')].map(rect)
      if (overviewHeads.length === 2 && close(rect(active.querySelector('.traffic-panel')).top, rect(active.querySelector('.group-panel')).top) && !close(overviewHeads[0].top, overviewHeads[1].top)) issues.push('Overview panel headings do not align')
      for (const selector of ['.mini-peer-main', '.mini-peer .connection-dot', '.mini-status', '.mini-traffic']) {
        const boxes = [...active.querySelectorAll(selector)].filter(visible).map(rect)
        if (boxes.some(box => !close(box.left, boxes[0].left) || !close(box.right, boxes[0].right))) issues.push(`${selector} columns do not align across rows`)
      }
      const footer = document.querySelector('.auth-footer'), card = document.querySelector('.auth-card')
      if (footer && card && rect(footer).top < rect(card).bottom + 16) issues.push('Authentication footer overlaps the card')
      const breadcrumbs = document.querySelector('.breadcrumbs'), headingTitle = active.querySelector('.page-heading h1')
      if (breadcrumbs && headingTitle && innerWidth > 700 && !close(rect(breadcrumbs).left, rect(headingTitle).left)) issues.push('Topbar and page content do not share a left edge')
      for (const row of active.querySelectorAll('.form-row')) {
        const boxes = [...row.querySelectorAll('input, select')].map(rect)
        if (boxes.some(box => !close(box.top, boxes[0].top) || !close(box.height, boxes[0].height) || !close(box.width, boxes[0].width))) issues.push('Form row fields do not align')
      }
      for (const selector of ['.peer-card', '.forward-card']) {
        const rows = [...active.querySelectorAll(selector)]
        const columns = selector === '.peer-card' ? ['.avatar', '.peer-identity', '.peer-meta-row', '.peer-traffic', '.peer-delete'] : ['.forward-identity', '.route-visual', '.forward-access', '.forward-card-actions']
        for (const column of columns) {
          const boxes = rows.map(row => row.querySelector(column)).filter(Boolean).map(rect)
          if (boxes.some(box => !close(box.left, boxes[0].left) || !close(box.right, boxes[0].right))) issues.push(`${column} columns do not align across rows`)
        }
      }
      for (const selector of ['.peer-traffic', '.route-visual']) {
        const rows = [...active.querySelectorAll(selector)]
        for (let i = 0; i < 2; i++) {
          const boxes = rows.map(row => selector === '.route-visual' ? row.querySelectorAll('b')[i] : row.children[i]).filter(Boolean).map(rect)
          if (boxes.some(box => !close(box.left, boxes[0].left))) issues.push(`${selector} value ${i + 1} does not share a column`)
        }
      }
      const dialog = document.querySelector('.dialog')
      if (dialog) {
        const box = rect(dialog)
        if (!close(box.left + box.width / 2, innerWidth / 2) || !close(box.top + box.height / 2, innerHeight / 2)) issues.push('Dialog is not centered in viewport')
        if (box.top < -1 || box.bottom > innerHeight + 1) issues.push('Dialog extends outside viewport vertically')
      }
      const heading = active.querySelector('.page-heading'), content = heading?.nextElementSibling
      return { issues: [...new Set(issues)], heading: heading ? { top: rect(heading).top + scrollY, height: rect(heading).height, next: content ? rect(content).top + scrollY : null } : null }
    })
    await page.screenshot({ path: resolve(output, `${name}.png`), fullPage: await page.locator('.dialog').count() === 0 })
    captures.push({ name, ...findings })
    if (/^\d+-(standard|stress)-(overview|peers|groups|forwards|settings)$/.test(name)) {
      const prefix = name.split('-').slice(0, 2).join('-')
      const first = captures.find(capture => capture.name === `${prefix}-overview`)
      if (first && findings.heading && Math.abs(first.heading.height - findings.heading.height) > 1) findings.issues.push('Page heading height changes between routes')
      if (first && findings.heading && Math.abs(first.heading.next - findings.heading.next) > 1) findings.issues.push('Content start position changes between routes')
    }
    for (const issue of findings.issues) failures.push(`${name}: ${issue}`)
  }
  console.log(`Visual artifacts: ${output}`)
  for (const width of [320, 390, 700, 720, 850, 980, 1024, 1250, 1440, 1920]) {
    await page.setViewportSize({ width, height: 900 })
    for (const mode of ['standard', 'stress']) {
      stress = mode === 'stress'; empty = fail = false; configured = true
      await page.goto(url); await login()
      for (const name of ['Overview', 'Peers', 'Groups', 'Forwards', 'Settings']) {
        await navigate(name); await capture(`${width}-${mode}-${name.toLowerCase()}`)
        if (name === 'Groups') {
          await page.locator('.detail-group-list button').first().click()
          await capture(`${width}-${mode}-group-details`)
          await page.locator('.react-flow__edge').first().click()
          await capture(`${width}-${mode}-link-details`)
        }
      }
    }
  }
  for (const width of [320, 390, 768, 1440]) {
    await page.setViewportSize({ width, height: 800 })
    stress = true; empty = fail = false; configured = true
    await page.goto(url); await page.getByLabel('Access token').waitFor(); await capture(`${width}-login`)
    await page.getByLabel('Access token').fill('invalid-token')
    await page.getByRole('button', { name: 'Connect', exact: true }).click()
    await page.getByRole('alert').waitFor(); await capture(`${width}-login-error`)
    await login()
    if (width <= 700) {
      await page.getByRole('button', { name: 'Open navigation' }).click(); await capture(`${width}-navigation`)
      await page.getByRole('navigation').getByRole('button', { name: 'Overview', exact: true }).click()
    }
    for (const name of ['Peers', 'Groups', 'Forwards']) {
      await navigate(name)
      await page.getByRole('button', { name: `New ${name === 'Peers' ? 'peer' : name === 'Groups' ? 'group' : 'forward'}`, exact: true }).first().click()
      await capture(`${width}-${name.toLowerCase()}-dialog`)
      if (name === 'Forwards') {
        await page.getByRole('dialog').getByLabel('Hub port', { exact: true }).fill('65535')
        await capture(`${width}-forward-validation`)
      }
      await page.keyboard.press('Escape')
    }
    await navigate('Forwards')
    let release, seen
    const requested = new Promise(resolve => { seen = resolve })
    configGate = { seen, released: new Promise(resolve => { release = resolve }) }
    await page.getByRole('button', { name: `Edit forward ${longName}`, exact: true }).click()
    await requested; await capture(`${width}-forward-loading`); release()
    await page.getByRole('dialog').getByLabel('Name', { exact: true }).waitFor()
    await page.waitForFunction(() => document.querySelector('.dialog input')?.value.length > 0)
    await capture(`${width}-forward-edit`)
    await page.setViewportSize({ width, height: 480 }); await capture(`${width}-short-forward-edit`)
    await page.getByRole('dialog').getByRole('button', { name: 'Save changes', exact: true }).scrollIntoViewIfNeeded()
    await capture(`${width}-short-forward-actions`)
    await page.setViewportSize({ width, height: 800 }); await page.keyboard.press('Escape')
    failConfig = true
    await page.getByRole('button', { name: `Edit forward ${longName}`, exact: true }).click()
    await page.getByRole('dialog').getByRole('alert').waitFor(); await capture(`${width}-forward-load-error`)
    failConfig = false; await page.keyboard.press('Escape')
    await navigate('Settings'); await page.getByLabel('Endpoint', { exact: true }).fill('invalid endpoint')
    await page.getByRole('button', { name: 'Save changes', exact: true }).click()
    await page.getByRole('alert').waitFor(); await capture(`${width}-settings-validation`)
    await navigate('Peers'); await page.getByLabel('Search peers').fill('no matching peer'); await capture(`${width}-search-empty`)
    await page.getByRole('button', { name: 'Clear search', exact: true }).first().click()
    await page.getByRole('button', { name: 'New peer', exact: true }).click()
    await page.getByRole('dialog').getByLabel('Name', { exact: true }).fill(longName)
    await page.getByRole('dialog').getByRole('button', { name: 'Create', exact: true }).click()
    await page.getByRole('heading', { name: 'Peer ready' }).waitFor()
    await page.locator('.config-details summary').click(); await capture(`${width}-provision`)
    await page.keyboard.press('Escape')
    await navigate('Groups'); await page.locator('.detail-group-list button').first().click()
    await page.getByRole('switch', { name: 'Intra-group access' }).check()
    await page.getByRole('button', { name: 'Save', exact: true }).click()
    await page.getByLabel('Policy conflict review').waitFor(); await capture(`${width}-policy-conflict`)
    empty = true
    await page.goto(url); await login()
    for (const name of ['Overview', 'Peers', 'Groups', 'Forwards']) { await navigate(name); await capture(`${width}-empty-${name.toLowerCase()}`) }
    empty = false; fail = true
    await page.goto(url); await login(); await navigate('Forwards'); await page.getByRole('alert').waitFor(); await capture(`${width}-error-forwards`)
    configured = false; fail = false
    await page.goto(url); await login(); await capture(`${width}-setup`)
    await page.getByLabel('Subnet', { exact: false }).fill('192.168.100.2/24')
    await page.getByLabel('Endpoint', { exact: true }).fill('vpn.example.com:51820')
    await page.getByRole('button', { name: 'Create network' }).click()
    await page.getByRole('alert').waitFor(); await capture(`${width}-setup-validation`)
    await page.setViewportSize({ width, height: 480 }); await capture(`${width}-short-setup`)
  }
  assert.deepEqual(errors, [], 'No runtime exceptions')
  await writeFile(resolve(output, 'report.json'), JSON.stringify({ captures, failures, errors }, null, 2))
  const escape = value => value.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('"', '&quot;')
  await writeFile(resolve(output, 'index.html'), `<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>WireHub visual review</title>
<style>body{margin:32px;font:14px system-ui;color:#17212f;background:#fafbfc}h1{font-size:26px}input{box-sizing:border-box;width:min(100%,500px);padding:12px;margin:12px 0 24px;border:1px solid #dbe3ee;border-radius:6px}main{display:grid;grid-template-columns:repeat(auto-fill,minmax(250px,1fr));gap:20px}figure{margin:0;padding:12px;border:1px solid #e8ecf1;border-radius:8px;background:white}figcaption{margin-bottom:10px;overflow-wrap:anywhere}img{width:100%;height:340px;object-fit:contain;object-position:top}a{color:inherit}</style>
<h1>WireHub visual review</h1><p>${captures.length} screens · 320–1920 px · Chrome ${browser.version()} · ${failures.length} geometry findings</p><p>Pages, group/link details, dialogs, validation, empty states, conflicts, long names, large counters and short windows. Select a screenshot to view it at full resolution.</p>
<input type="search" placeholder="Filter, e.g. 320, stress, dialog, short" aria-label="Filter screenshots"><main>${captures.map(capture => `<figure data-name="${capture.name}"><figcaption>${capture.name}${capture.issues.length ? ' · ' + escape(capture.issues.join('; ')) : ''}</figcaption><a href="${capture.name}.png"><img src="${capture.name}.png" loading="lazy" alt="${capture.name}"></a></figure>`).join('')}</main>
<script>document.querySelector('input').addEventListener('input',event=>{for(const figure of document.querySelectorAll('figure'))figure.hidden=!figure.dataset.name.includes(event.target.value.toLowerCase())})</script></html>`)
  console.log(`${captures.length} screens checked; ${failures.length} alignment findings`)
  for (const failure of failures) console.error(failure)
  assert.deepEqual(failures, [], 'All visual alignment checks pass')
} finally {
  await browser?.close()
  await new Promise(resolve => server.close(resolve))
}
