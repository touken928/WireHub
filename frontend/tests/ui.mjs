import assert from 'node:assert/strict'
import { createServer } from 'node:http'
import { readFile, mkdir } from 'node:fs/promises'
import { resolve, extname } from 'node:path'
import { chromium } from 'playwright'

// Exercise the production bundle against isolated API fixtures; never touch a running hub.
const dist = resolve(import.meta.dirname, '../dist')
const screenshots = process.env.WIREHUB_UI_SCREENSHOTS
const server = createServer(async (req, res) => {
  try {
    const pathname = new URL(req.url, 'http://localhost').pathname
    const file = pathname.startsWith('/assets/') ? resolve(dist, `.${pathname}`) : resolve(dist, 'index.html')
    if (!file.startsWith(dist + '/')) { res.writeHead(403).end(); return }
    const body = await readFile(file)
    res.setHeader('Content-Type', ({ '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css' })[extname(file)] ?? 'application/octet-stream')
    res.end(body)
  } catch { res.writeHead(404).end() }
})
await new Promise(r => server.listen(0, '127.0.0.1', r))
const url = `http://127.0.0.1:${server.address().port}`
let browser
try {
  browser = await chromium.launch({ channel: process.env.PLAYWRIGHT_CHANNEL ?? 'chrome', headless: true })
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 }, reducedMotion: 'reduce' })
  const runtimeErrors = []
  page.on('pageerror', error => runtimeErrors.push(error.message))
  page.on('dialog', dialog => dialog.accept())
  let groups = [
    { id: 'engineering', name: 'Engineering', allowed_groups: [] },
    { id: 'operations', name: 'Operations', allowed_groups: [] },
    { id: 'services', name: 'Services', allowed_groups: [] },
  ]
  let peers = [
    { id: 'mac', name: 'MacBook Pro', group_id: 'engineering', ipv4: '10.77.0.2', public_key: 'fixture', sent_bytes: 1820000, received_bytes: 4600000, last_handshake_unix: Math.floor(Date.now() / 1000) },
    { id: 'server', name: 'Build server', group_id: 'services', ipv4: '10.77.0.3', public_key: 'fixture', sent_bytes: 65400000, received_bytes: 87600000, last_handshake_unix: null },
  ]
  let forwards = [{ id: 'docs', name: 'Internal docs', protocol: 'tcp', target_port: 8080, target_peer_id: 'server', allowed_group_ids: ['engineering'] }]
  let settings = { subnet: '10.77.0.0/24', endpoint: 'vpn.example.com:51820', persistent_keepalive: 25 }
  let configured = true, failSave = false, failReload = false, partialSave = false
  let failSettings = false
  let setupCreateStatus = 200, setupCreateError = ''
  const mutations = []
  await page.route('**/api/**', async route => {
    const req = route.request(), path = new URL(req.url()).pathname, method = req.method(), body = req.postDataJSON()
    const reply = (json, status = 200) => route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(json) })
    const textError = (text, status) => route.fulfill({ status, contentType: 'text/plain; charset=utf-8', body: text })
    if (req.headers().authorization !== 'Bearer ui-test-token') return textError('unauthorized', 401)
    if (method !== 'GET') mutations.push({ method, path, body })
    if (path === '/api/setup') {
      if (method === 'POST') {
        if (setupCreateStatus !== 200) {
          if (setupCreateStatus === 409) { configured = true; settings = { subnet: '10.20.30.0/24', endpoint: 'saved.example.com:51820', persistent_keepalive: 25 } }
          if (setupCreateStatus === 503) { configured = true; settings = body }
          return textError(setupCreateError, setupCreateStatus)
        }
        settings = body; configured = true; return reply(settings)
      }
      return reply({ configured, settings: configured ? settings : null })
    }
    if (path === '/api/settings') {
      if (failSettings) return textError('setup required', 409)
      settings = { ...settings, ...body }; return reply(settings)
    }
    if (path === '/api/groups' && method === 'GET') return failReload ? reply({}, 503) : reply(groups)
    if (path.endsWith('/acl')) {
      if (failSave || (partialSave && path.includes('/operations/'))) return reply({}, 503)
      const group = groups.find(g => g.id === path.split('/')[3])
      group.allowed_groups = body.allowed_groups
      return reply(group)
    }
    if (path === '/api/groups' && method === 'POST') { const group = { id: 'new-group', ...body, allowed_groups: [] }; groups.push(group); return reply(group) }
    if (path.startsWith('/api/groups/') && method === 'DELETE') { const id = path.split('/')[3]; groups = groups.filter(g => g.id !== id); groups.forEach(g => { g.allowed_groups = g.allowed_groups.filter(to => to !== id) }); forwards = forwards.map(f => ({ ...f, allowed_group_ids: f.allowed_group_ids.filter(to => to !== id) })); return route.fulfill({ status: 204 }) }
    if (path === '/api/peers' && method === 'GET') return reply(peers)
    if (path === '/api/peers' && method === 'POST') { const peer = { ...peers[0], ...body, id: 'new-peer', ipv4: '10.77.0.4' }; peers.push(peer); return reply({ peer, config: '[Interface]\nPrivateKey = fixture-only\nAddress = 10.77.0.4/32' }) }
    if (path.endsWith('/group')) { const peer = peers.find(p => p.id === path.split('/')[3]); peer.group_id = body.group_id; return reply(peer) }
    if (path.startsWith('/api/peers/') && method === 'DELETE') { const id = path.split('/')[3]; peers = peers.filter(p => p.id !== id); forwards = forwards.filter(f => f.target_peer_id !== id); return route.fulfill({ status: 204 }) }
    if (path === '/api/forwards' && method === 'GET') return reply(forwards)
    if (path === '/api/forwards' && method === 'POST') { const forward = { id: 'new-forward', ...body }; forwards.push(forward); return reply(forward) }
    if (path.startsWith('/api/forwards/') && method === 'DELETE') { forwards = forwards.filter(f => f.id !== path.split('/')[3]); return route.fulfill({ status: 204 }) }
    return reply({}, 404)
  })
  const screenshot = async name => { if (screenshots) { await mkdir(screenshots, { recursive: true }); await page.screenshot({ path: resolve(screenshots, `${name}.png`), fullPage: true }) } }
  const english = async () => {
    const content = await page.evaluate(() => document.body.innerText + [...document.querySelectorAll('[aria-label], [placeholder], [title]')].map(e => [e.getAttribute('aria-label'), e.getAttribute('placeholder'), e.getAttribute('title')].join(' ')).join(' '))
    assert.ok(!/[\p{Script=Han}]/u.test(content), 'All UI text and accessible labels must be English')
    assert.equal(await page.locator('html').getAttribute('lang'), 'en')
  }
  const noOverflow = async () => assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), 'No page-level horizontal overflow')
  const login = async () => { await page.getByLabel('Access token').fill('ui-test-token'); await page.getByRole('button', { name: 'Connect', exact: true }).click(); await page.getByRole('heading', { name: 'Overview', exact: true }).waitFor() }
  const groupsVisible = async () => {
    const canvas = await page.locator('.graph-canvas').boundingBox()
    const boxes = await Promise.all(groups.map(g => node(g.id).boundingBox()))
    return boxes.every(b => b && b.x >= canvas.x && b.y >= canvas.y && b.x + b.width <= canvas.x + canvas.width && b.y + b.height <= canvas.y + canvas.height)
  }
  const navigate = async name => {
    const open = page.getByRole('button', { name: 'Open navigation', exact: true })
    if (await open.isVisible()) await open.click()
    await page.getByRole('navigation').getByRole('button', { name, exact: name === 'Groups' || name === 'Settings' || name === 'Overview' }).click()
    await page.getByRole('heading', { name, exact: true, level: 1 }).waitFor()
    if (name === 'Groups' && groups.length) await poll(groupsVisible, 'Groups must fit the canvas on entry')
  }
  const node = id => page.locator(`.react-flow__node[data-id="${id}"]`)
  const edge = () => page.locator('.react-flow__edge')
  const poll = async (predicate, message) => { for (let i = 0; i < 50; i++) { if (await predicate()) return; await page.waitForTimeout(100) } assert.fail(message) }
  const connect = async (from, to) => {
    const a = await node(from).boundingBox(), b = await node(to).boundingBox()
    const dx = b.x - a.x, dy = b.y - a.y
    const side = Math.abs(dx) > Math.abs(dy) ? (dx > 0 ? 'right' : 'left') : (dy > 0 ? 'bottom' : 'top')
    const opposite = { right: 'left', left: 'right', top: 'bottom', bottom: 'top' }[side]
    const start = await node(from).locator(`[data-handleid="${side}"]`).boundingBox()
    const end = await node(to).locator(`[data-handleid="${opposite}"]`).boundingBox()
    await page.mouse.move(start.x + start.width / 2, start.y + start.height / 2)
    await page.mouse.down()
    await page.mouse.move(end.x + end.width / 2, end.y + end.height / 2, { steps: 20 })
    await page.mouse.up()
  }
  const save = async () => { await page.getByRole('button', { name: 'Save', exact: true }).click(); await page.getByText('Policy canvas', { exact: true }).waitFor(); await poll(async () => await page.getByRole('button', { name: 'Saving', exact: true }).count() === 0, 'Save settles'); await page.getByText('Policy saved', { exact: true }).waitFor() }
  await page.goto(url); await english(); await screenshot('login')
  await page.getByLabel('Access token').fill('wrong-token'); await page.getByRole('button', { name: 'Connect', exact: true }).click(); await page.getByRole('alert').waitFor(); assert.equal(await page.getByRole('alert').innerText(), 'unauthorized'); await english()
  await login(); await english(); await noOverflow(); await screenshot('overview')
  await navigate('Groups'); await node('engineering').waitFor(); await page.waitForTimeout(100); await screenshot('groups'); await english()
  await node('engineering').click(); await page.getByLabel('Close group details').click()
  await poll(async () => await page.getByLabel('Close group details').count() === 0, 'Group details stay closed after X')
  await poll(async () => !(await node('engineering').getAttribute('class')).includes('selected'), 'Closing details clears node selection'); assert.equal(await page.getByLabel('Close group details').count(), 0)
  await node('services').focus(); await page.keyboard.press('Enter'); await page.getByLabel('Close group details').waitFor()
  await page.keyboard.press('Escape'); await poll(async () => await page.getByLabel('Close group details').count() === 0, 'Escape clears group details')
  await page.locator('.detail-group-list').getByRole('button', { name: 'Operations' }).click(); await page.getByLabel('Close group details').click()
  await poll(async () => await page.getByLabel('Close group details').count() === 0, 'Details selected from the list also close')
  // A drag from the source permits initiation in only that direction.
  await page.getByRole('group', { name: 'New link direction' }).getByRole('button', { name: 'One way', exact: true }).click()
  await connect('engineering', 'operations'); await poll(async () => await edge().count() === 1, 'One-way drag creates an edge')
  assert.match(await edge().getAttribute('aria-label'), /^Engineering → Operations$/)
  await save(); assert.deepEqual(groups[0].allowed_groups, ['operations']); assert.deepEqual(groups[1].allowed_groups, [])
  // Duplicate and self gestures never create a second edge or permission.
  await connect('engineering', 'operations'); await connect('engineering', 'engineering'); assert.equal(await edge().count(), 1)
  assert.ok(await page.getByRole('button', { name: 'Save', exact: true }).isDisabled())
  // Reverse initiation upgrades a one-way link to a single bidirectional edge.
  await connect('operations', 'engineering'); await poll(async () => /↔/.test(await edge().getAttribute('aria-label')), 'Reverse gesture creates bidirectional access')
  await save(); assert.deepEqual(groups[1].allowed_groups, ['engineering']); assert.equal(await edge().count(), 1)
  // Live nearest-side handles rematch as a group crosses its connected neighbor.
  await node('engineering').click()
  const before = await node('engineering').getAttribute('style'), previousPath = await edge().locator('.react-flow__edge-path').getAttribute('d')
  const box = await node('engineering').boundingBox()
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2); await page.mouse.down(); await page.mouse.move(box.x + 180, box.y + 180, { steps: 20 }); await page.mouse.up()
  assert.notEqual(await node('engineering').getAttribute('style'), before)
  assert.notEqual(await edge().locator('.react-flow__edge-path').getAttribute('d'), previousPath)
  assert.ok(await page.getByRole('button', { name: 'Save', exact: true }).isDisabled(), 'Moving nodes must not mutate ACLs')
  const layout = await page.evaluate(() => JSON.parse(localStorage.getItem('wirehub.group-layout.v1')))
  assert.ok(Number.isFinite(layout.engineering.x))
  await navigate('Overview'); await navigate('Groups')
  await poll(async () => { const style = await node('engineering').getAttribute('style'); const x = /translate\(([-\d.]+)px/.exec(style)?.[1]; return Math.abs(Number(x) - layout.engineering.x) < .01 }, 'Layout survives page navigation')
  // Edge inspector changes direction, and keyboard deletion removes its rules.
  await edge().focus(); await page.keyboard.press('Enter'); await page.getByRole('heading', { name: 'Access direction' }).waitFor()
  await page.getByRole('button', { name: 'Reverse', exact: true }).click(); await save()
  assert.deepEqual(groups[0].allowed_groups, []); assert.deepEqual(groups[1].allowed_groups, ['engineering'])
  await edge().focus(); await page.keyboard.press('Delete', { delay: 50 }); await poll(async () => await edge().count() === 0, 'Delete disconnects selected edge'); await save()
  assert.deepEqual(groups[1].allowed_groups, [])
  // Explicit self-access is separate from graph connections.
  await node('engineering').click(); await page.getByRole('switch', { name: 'Intra-group access' }).check(); await save()
  assert.deepEqual(groups[0].allowed_groups, ['engineering']); assert.equal(await edge().count(), 0)
  await page.getByRole('switch', { name: 'Intra-group access' }).uncheck(); await page.getByRole('button', { name: 'Discard changes' }).click()
  assert.ok(await page.getByRole('switch', { name: 'Intra-group access' }).isChecked())
  await page.getByRole('button', { name: 'Auto layout' }).click(); await page.waitForTimeout(150)
  // A partial two-group save reloads the authoritative server state.
  await page.getByRole('group', { name: 'New link direction' }).getByRole('button', { name: 'Both ways', exact: true }).click()
  await connect('engineering', 'operations'); await poll(async () => await edge().count() === 1, 'Connection after auto layout'); partialSave = true
  await page.getByRole('button', { name: 'Save', exact: true }).click(); await page.getByRole('alert').filter({ hasText: 'Server policy reloaded' }).waitFor()
  assert.deepEqual(groups[0].allowed_groups, ['engineering', 'operations']); assert.deepEqual(groups[1].allowed_groups, [])
  assert.match(await edge().getAttribute('aria-label'), /→/); partialSave = false
  // Unconfirmed saves lock editing until a successful reload.
  await connect('operations', 'engineering'); failSave = true; failReload = true
  await page.getByRole('button', { name: 'Save', exact: true }).click(); await page.getByRole('button', { name: 'Reload policy' }).waitFor()
  assert.ok(await page.getByRole('button', { name: 'New group', exact: true }).isDisabled()); failSave = false; failReload = false
  await page.getByRole('button', { name: 'Reload policy' }).click(); await poll(async () => await page.getByRole('alert').count() === 0, 'Reload clears uncertainty')
  assert.match(await edge().getAttribute('aria-label'), /→/)
  // Group creation and deletion use the existing API contract.
  await page.getByRole('button', { name: 'New group', exact: true }).click(); assert.ok(await page.getByRole('dialog').getByLabel('Name', { exact: true }).evaluate(el => document.activeElement === el), 'New group focuses its name'); await page.getByRole('dialog').getByLabel('Name', { exact: true }).fill('Research'); await page.getByRole('button', { name: 'Create', exact: true }).click(); await node('new-group').waitFor()
  await node('new-group').click(); await page.getByRole('button', { name: 'Delete group', exact: true }).click(); await poll(async () => await node('new-group').count() === 0, 'Group deleted')
  await english(); await screenshot('groups-connected')
  await page.setViewportSize({ width: 390, height: 844 }); await poll(groupsVisible, 'Canvas fits after resizing'); await noOverflow()
  await page.setViewportSize({ width: 1440, height: 1000 }); await poll(groupsVisible, 'Canvas fits after expanding')
  await navigate('Peers')
  const peerBoxes = await page.locator('.peer-card').evaluateAll(cards => cards.map(card => { const b = card.getBoundingClientRect(); return { width: b.width, height: b.height, x: b.x, y: b.y } }))
  assert.ok(peerBoxes.every(b => b.height < 120 && b.width > b.height * 3), 'Peers use compact horizontal rows')
  assert.ok(peerBoxes[1].y > peerBoxes[0].y && peerBoxes[1].x === peerBoxes[0].x, 'Peer rows form a vertical list')
  await page.getByLabel('Search peers').fill('10.77.0.2'); assert.equal(await page.locator('.peer-card').count(), 1); await page.getByLabel('Clear search').click()
  await page.getByLabel('Group for MacBook Pro').selectOption('operations'); await poll(() => peers[0].group_id === 'operations', 'Peer moved')
  await page.getByRole('button', { name: 'New peer', exact: true }).click(); await page.getByRole('dialog').getByLabel('Name', { exact: true }).fill('Test laptop'); await page.getByRole('button', { name: 'Create', exact: true }).click(); await page.getByRole('heading', { name: 'Peer ready' }).waitFor(); await english()
  const download = page.waitForEvent('download'); await page.getByRole('button', { name: 'Download', exact: true }).click(); assert.equal((await download).suggestedFilename(), 'Test-laptop.conf')
  await page.getByRole('button', { name: 'Saved. Close.' }).click(); await page.getByLabel('Delete peer Test laptop').click(); await poll(async () => peers.length === 2 && await page.locator('.peer-card').count() === 2, 'Peer deleted'); await screenshot('peers')
  await navigate('Forwards'); await page.getByRole('button', { name: 'New forward', exact: true }).click(); await page.getByRole('dialog').getByLabel('Name', { exact: true }).fill('Test service'); await page.getByRole('dialog').getByLabel('Port', { exact: true }).fill('8080'); await page.getByRole('alert').waitFor(); assert.ok(await page.getByRole('button', { name: 'Create', exact: true }).isDisabled())
  await page.getByRole('dialog').getByLabel('Protocol', { exact: true }).selectOption('udp'); await page.getByRole('dialog').getByLabel('Engineering', { exact: false }).check(); await page.getByRole('button', { name: 'Create', exact: true }).click(); await poll(async () => forwards.length === 2 && await page.getByRole('dialog').count() === 0 && await page.locator('.forward-card').count() === 2, 'Forward created'); await english(); await screenshot('forwards')
  await page.getByLabel('Delete forward Test service').click(); await poll(() => forwards.length === 1, 'Forward deleted')
  await navigate('Settings'); await page.getByLabel('Endpoint', { exact: true }).fill('vpn2.example.com:51820'); await page.getByRole('button', { name: 'Save changes', exact: true }).click(); await poll(async () => settings.endpoint === 'vpn2.example.com:51820' && await page.getByRole('button', { name: 'Save changes', exact: true }).count() === 1, 'Defaults saved'); await english(); await screenshot('settings')
  // Settings errors use the backend's plain-text response contract too.
  failSettings = true; await page.getByLabel('Endpoint', { exact: true }).fill('vpn3.example.com:51820'); await page.getByRole('button', { name: 'Save changes', exact: true }).click(); await page.getByRole('alert').filter({ hasText: 'setup required' }).waitFor(); failSettings = false
  // Narrow layout: all pages and dialogs remain usable without horizontal overflow.
  await page.setViewportSize({ width: 390, height: 844 })
  for (const name of ['Overview', 'Peers', 'Groups', 'Forwards', 'Settings']) {
    await navigate(name); await noOverflow(); await english()
    if (name === 'Peers') assert.ok(await page.locator('.peer-card').evaluateAll(cards => cards.every(card => card.getBoundingClientRect().height < 170)), 'Mobile peer rows stay compact')
    if (name === 'Groups') { await node('engineering').click(); await page.getByLabel('Close group details').click(); await poll(async () => await page.getByLabel('Close group details').count() === 0, 'Group details close on mobile') }
    await screenshot(`mobile-${name.toLowerCase()}`)
  }
  // Operations has a real incoming ACL reference to Engineering, matching the backend's cascade case.
  // Clear the existing edge and create the incoming rule through the UI before deletion.
  await navigate('Groups')
  while (await edge().count()) { await edge().click(); await edge().focus(); await page.keyboard.press('Delete', { delay: 50 }); await poll(async () => await edge().count() === 0, 'Existing edge removed') }
  await save(); await connect('operations', 'engineering'); await poll(async () => await edge().count() === 1 && (await edge().getAttribute('aria-label')).includes('Operations'), 'Incoming Operations → Engineering ACL is visible'); await save()
  // Both Engineering peers were moved/deleted above, so backend group-deletion restrictions permit the request.
  assert.deepEqual(groups.find(g => g.id === 'operations').allowed_groups, ['engineering'])
  await node('engineering').click(); await page.getByRole('button', { name: 'Delete group', exact: true }).click()
  await poll(async () => !groups.some(g => g.id === 'engineering') && !forwards[0].allowed_group_ids.includes('engineering') && await node('engineering').count() === 0 && await edge().count() === 0, 'Group deletion cascades allowlists')
  assert.deepEqual(groups.find(g => g.id === 'operations').allowed_groups, [], 'Server cascade removes incoming ACL reference')
  assert.equal(await page.locator('.react-flow__edge').count(), 0, 'Local policy graph drops the incoming ACL edge')
  await navigate('Forwards'); assert.equal(await page.locator('.allow-chips').innerText(), 'No access')
  // A peer deletion cascades its dependent forwards in both server fixtures and UI state, freeing its port.
  await navigate('Peers'); await page.getByLabel('Delete peer Build server').click(); await poll(async () => peers.length === 1 && forwards.length === 0 && await page.locator('.peer-card').count() === 1, 'Peer deletion cascades target forwards')
  await navigate('Forwards'); assert.equal(await page.locator('.forward-card').count(), 0)
  await page.getByRole('button', { name: 'New forward', exact: true }).first().click(); await page.getByRole('dialog').getByLabel('Name', { exact: true }).fill('Reused port'); await page.getByRole('dialog').getByLabel('Port', { exact: true }).fill('8080'); await page.getByRole('dialog').getByLabel('Target peer', { exact: true }).selectOption('mac'); await page.getByRole('button', { name: 'Create', exact: true }).click(); await poll(() => forwards.length === 1 && forwards[0].target_peer_id === 'mac', 'Cascade frees the forward port for reuse')
  await navigate('Groups'); await page.getByRole('button', { name: 'New group', exact: true }).click(); await noOverflow(); await english(); await screenshot('mobile-dialog'); await page.keyboard.press('Escape')
  // Fresh setup, empty states, session reset, and no persisted access token.
  await page.getByRole('button', { name: 'Open navigation' }).click(); await page.getByRole('button', { name: 'Administrator Sign out' }).click(); await page.getByLabel('Access token').waitFor()
  configured = false; groups = []; peers = []; forwards = []
  await page.getByLabel('Access token').fill('ui-test-token'); await page.getByRole('button', { name: 'Connect', exact: true }).click(); await page.getByRole('heading', { name: 'Create your network.' }).waitFor(); assert.equal(await page.getByLabel('Subnet', { exact: false }).inputValue(), '10.10.10.0/24'); await english(); await noOverflow(); await screenshot('mobile-setup')
  setupCreateStatus = 409; setupCreateError = 'setup already completed'
  await page.getByLabel('Endpoint', { exact: true }).fill('vpn.example.com:51820'); await page.getByRole('button', { name: 'Create network' }).click(); await page.getByRole('heading', { name: 'Overview', exact: true }).waitFor(); assert.equal(settings.subnet, '10.20.30.0/24', 'A 409 reconciles by reading saved setup status')
  await page.getByRole('button', { name: 'Open navigation' }).click(); await page.getByRole('button', { name: 'Administrator Sign out' }).click(); await page.getByLabel('Access token').waitFor(); configured = false; setupCreateStatus = 503; setupCreateError = 'settings saved, but runtime activation was not acknowledged; inspect setup status and restart the service before provisioning'
  await page.getByLabel('Access token').fill('ui-test-token'); await page.getByRole('button', { name: 'Connect', exact: true }).click(); await page.getByRole('heading', { name: 'Create your network.' }).waitFor(); await page.getByLabel('Endpoint', { exact: true }).fill('vpn.example.com:51820'); await page.getByRole('button', { name: 'Create network' }).click(); assert.equal(await page.getByRole('alert').innerText(), 'settings saved, but runtime activation was not acknowledged; inspect setup status and restart the service before provisioning')
  assert.equal(configured, true, 'The service persists setup before an activation failure response')
  await page.reload(); await page.getByLabel('Access token').waitFor()
  await page.getByLabel('Access token').fill('ui-test-token'); await page.getByRole('button', { name: 'Connect', exact: true }).click(); await page.getByRole('heading', { name: 'Overview', exact: true }).waitFor(); assert.equal(settings.endpoint, 'vpn.example.com:51820', 'Reload recovers setup persisted before activation failed')
  await page.getByRole('button', { name: 'Open navigation' }).click(); await page.getByRole('button', { name: 'Administrator Sign out' }).click(); await page.getByLabel('Access token').waitFor()
  configured = false; setupCreateStatus = 200; setupCreateError = ''; await page.reload(); await page.getByLabel('Access token').waitFor()
  await page.getByLabel('Access token').fill('ui-test-token'); await page.getByRole('button', { name: 'Connect', exact: true }).click(); await page.getByRole('heading', { name: 'Create your network.' }).waitFor();
  await page.getByLabel('Endpoint', { exact: true }).fill('vpn.example.com:51820'); await page.getByRole('button', { name: 'Create network' }).click(); await page.getByRole('heading', { name: 'Overview', exact: true }).waitFor(); assert.equal(settings.subnet, '10.10.10.0/24'); assert.equal(mutations.find(m => m.path === '/api/setup' && m.method === 'POST').body.subnet, '10.10.10.0/24')
  for (const name of ['Overview', 'Peers', 'Groups', 'Forwards']) { await navigate(name); await english(); await noOverflow() }
  await page.reload(); await page.getByLabel('Access token').waitFor(); assert.equal(await page.getByLabel('Access token').inputValue(), '')
  assert.deepEqual(runtimeErrors, [], 'No runtime exceptions')
  assert.ok(mutations.every(m => m.path.startsWith('/api/')), 'Existing API paths only')
  console.log('PASS: group details closing, compact peer rows, graph direction, drag, persistence, deletion, self-access, partial saves, recovery, CRUD, English, mobile, setup, and session reset')
} finally {
  await browser?.close()
  await new Promise(r => server.close(r))
}
