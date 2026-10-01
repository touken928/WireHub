import assert from 'node:assert/strict'
import { createServer } from 'node:http'
import { readFile, mkdir, writeFile } from 'node:fs/promises'
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
let browser, page
try {
  browser = await chromium.launch({ channel: process.env.PLAYWRIGHT_CHANNEL ?? 'chrome', headless: true })
  console.log(`UI fixture: production bundle, ${process.env.PLAYWRIGHT_CHANNEL ?? 'chrome'} ${browser.version()}, isolated loopback API, desktop 1440×1000 / mobile 390×844`)
  page = await browser.newPage({ viewport: { width: 1440, height: 1000 }, reducedMotion: 'reduce' })
  // The clock keeps ticking normally (no pause/frozen RAF). Only the toast-miss
  // regression advances time, after the real mutation and clean UI settle.
  await page.clock.install()
  if (screenshots) await page.context().tracing.start({ screenshots: true, snapshots: true, sources: true })
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
  // Capture the response at request time, then explicitly release it after a mutation.
  // No timing sleeps: each race is ordered by the request/response barriers below.
  const heldResponses = new Map()
  const holdNext = (method, path) => {
    const key = `${method} ${path}`
    assert.ok(!heldResponses.has(key), `Only one pending barrier for ${key}`)
    let seen, release
    const requested = new Promise(resolve => { seen = resolve })
    const released = new Promise(resolve => { release = resolve })
    heldResponses.set(key, { seen, released })
    return { requested, release: async () => {
      const response = page.waitForResponse(r => r.request().method() === method && new URL(r.url()).pathname === path)
      release()
      await (await response).finished()
    } }
  }
  await page.route('**/api/**', async route => {
    const req = route.request(), path = new URL(req.url()).pathname, method = req.method(), body = req.postDataJSON()
    const respond = async response => {
      const key = `${method} ${path}`, held = heldResponses.get(key)
      if (held) { heldResponses.delete(key); held.seen(); await held.released }
      return route.fulfill(response)
    }
    const reply = (json, status = 200) => respond({ status, contentType: 'application/json', body: JSON.stringify(json) })
    const textError = (text, status) => respond({ status, contentType: 'text/plain; charset=utf-8', body: text })
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
  const sorted = values => [...values].sort()
  const fixtureAcl = () => Object.fromEntries(groups.map(g => [g.id, sorted(g.allowed_groups)]))
  const graphAcl = async () => {
    const allowed = Object.fromEntries(groups.map(g => [g.id, []]))
    const ids = new Map(groups.map(g => [g.name, g.id]))
    for (const g of groups) {
      if (await node(g.id).getByLabel('Intra-group access allowed', { exact: true }).count()) allowed[g.id].push(g.id)
    }
    for (const label of await edge().evaluateAll(edges => edges.map(e => e.getAttribute('aria-label')))) {
      const [from, direction, to] = label.split(/ (→|↔) /)
      assert.ok(ids.has(from) && ids.has(to), `Known groups in ${label}`)
      allowed[ids.get(from)].push(ids.get(to))
      if (direction === '↔') allowed[ids.get(to)].push(ids.get(from))
    }
    return Object.fromEntries(Object.entries(allowed).map(([id, values]) => [id, sorted(values)]))
  }
  const save = async (changes, { checkToast = false, expireToast = false } = {}) => {
    // Capture the intended ACL before the fixture mutates. A clean canvas alone
    // can also mean a failed save reloaded old rules, or a stale draft was lost.
    const expected = { ...fixtureAcl(), ...Object.fromEntries(Object.entries(changes).map(([id, values]) => [id, sorted(values)])) }
    await poll(async () => JSON.stringify(await graphAcl()) === JSON.stringify(expected), 'Draft graph matches intended ACL before save')
    const changed = groups.filter(g => JSON.stringify(sorted(g.allowed_groups)) !== JSON.stringify(expected[g.id]))
    assert.ok(changed.length, 'Save must have an actual ACL mutation')
    assert.ok(await page.getByRole('button', { name: 'Save', exact: true }).isEnabled(), 'Dirty policy enables Save')
    await page.getByText('Unsaved changes', { exact: true }).waitFor()
    // Register before clicking, and match this save's PUT payload, never an old
    // GET or a toast left over from a previous successful save.
    const responses = changed.map(g => page.waitForResponse(r => r.request().method() === 'PUT'
      && new URL(r.url()).pathname === `/api/groups/${g.id}/acl`
      && JSON.stringify(sorted(r.request().postDataJSON().allowed_groups)) === JSON.stringify(expected[g.id])))
    const toast = checkToast ? page.getByText('Policy saved', { exact: true }).waitFor() : null
    const held = expireToast ? changed.map(g => holdNext('PUT', `/api/groups/${g.id}/acl`)) : []
    const [_, ...confirmed] = await Promise.all([
      (async () => {
        await page.getByRole('button', { name: 'Save', exact: true }).click()
        if (held.length) {
          await Promise.all(held.map(h => h.requested))
          assert.ok(await page.getByRole('button', { name: 'Saving', exact: true }).isDisabled(), 'Held mutation keeps Save busy')
          assert.ok(await page.getByRole('switch', { name: 'Intra-group access' }).isDisabled(), 'Held mutation locks ACL editing')
          await page.getByText('Unsaved changes', { exact: true }).waitFor()
          await Promise.all(held.map(h => h.release()))
        }
      })(),
      ...responses,
    ])
    for (const [index, response] of confirmed.entries()) {
      assert.equal(await response.finished(), null, 'ACL response completes without network error')
      assert.equal(response.status(), 200, 'This ACL mutation succeeded')
      const group = await response.json()
      assert.equal(group.id, changed[index].id)
      assert.deepEqual(sorted(group.allowed_groups), expected[group.id], 'Mutation response confirms intended ACL')
    }
    assert.deepEqual(fixtureAcl(), expected, 'Authoritative fixture confirms every group ACL')
    await page.waitForFunction(() => {
      const button = [...document.querySelectorAll('.graph-actions button')].find(b => b.textContent === 'Save')
      return button?.disabled && document.querySelector('.graph-status')?.textContent === 'Policy canvas'
        && !document.querySelector('[aria-label="Discard changes"]')
    })
    if (toast) await toast // Keep explicit success-notification coverage on the first save.
    if (expireToast) {
      // Deterministically model a delayed test observer, not a slower server.
      // Advance the real 2800 ms toast deadline without a wall-clock sleep.
      await page.clock.runFor(2801)
      assert.equal(await page.getByText('Policy saved', { exact: true }).count(), 0, 'Regression observes the save after its toast expires')
    }
    assert.ok(await page.getByRole('button', { name: 'Save', exact: true }).isDisabled(), 'Confirmed policy remains clean after settlement')
    assert.equal(await page.getByRole('alert').count(), 0, 'Successful save has no reload/error alert')
    assert.deepEqual(await graphAcl(), expected, 'Persistent UI matches authoritative saved ACL, including self-access')
  }
  const refresh = page.getByRole('button', { name: 'Refresh data', exact: true })
  const refreshSettled = () => page.waitForFunction(() => !document.querySelector('[aria-label="Refresh data"]').disabled)
  const staleRefresh = async path => {
    await refreshSettled()
    const held = holdNext('GET', path)
    await refresh.click(); await held.requested
    assert.ok(await refresh.isDisabled(), 'Refresh is busy until the held read settles')
    return async () => { await held.release(); await refreshSettled() }
  }
  const flushFrames = () => page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))))
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
  await save({ engineering: ['operations'] }, { checkToast: true }); assert.deepEqual(groups[0].allowed_groups, ['operations']); assert.deepEqual(groups[1].allowed_groups, [])
  // Duplicate and self gestures never create a second edge or permission.
  await connect('engineering', 'operations'); await connect('engineering', 'engineering'); assert.equal(await edge().count(), 1)
  assert.ok(await page.getByRole('button', { name: 'Save', exact: true }).isDisabled())
  // Reverse initiation upgrades a one-way link to a single bidirectional edge.
  await connect('operations', 'engineering'); await poll(async () => /↔/.test(await edge().getAttribute('aria-label')), 'Reverse gesture creates bidirectional access')
  await save({ operations: ['engineering'] }); assert.deepEqual(groups[1].allowed_groups, ['engineering']); assert.equal(await edge().count(), 1)
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
  await page.getByRole('button', { name: 'Reverse', exact: true }).click(); await save({ engineering: [] })
  assert.deepEqual(groups[0].allowed_groups, []); assert.deepEqual(groups[1].allowed_groups, ['engineering'])
  await edge().focus(); await page.keyboard.press('Delete', { delay: 50 }); await poll(async () => await edge().count() === 0, 'Delete disconnects selected edge'); await save({ operations: [] })
  assert.deepEqual(groups[1].allowed_groups, [])
  // Explicit self-access is separate from graph connections.
  await node('engineering').click(); await page.getByRole('switch', { name: 'Intra-group access' }).check(); await save({ engineering: ['engineering'] })
  assert.deepEqual(groups[0].allowed_groups, ['engineering']); assert.equal(await edge().count(), 0)
  await page.getByRole('switch', { name: 'Intra-group access' }).uncheck(); await page.getByRole('button', { name: 'Discard changes' }).click()
  assert.ok(await page.getByRole('switch', { name: 'Intra-group access' }).isChecked())
  // Regression: a refresh started with permission=true must not restore it after a confirmed revoke.
  await refreshSettled()
  const oldAcl = holdNext('GET', '/api/groups')
  await refresh.click(); await oldAcl.requested
  assert.ok(await refresh.isDisabled(), 'Refresh remains busy while its response is held')
  await page.getByRole('switch', { name: 'Intra-group access' }).uncheck(); await save({ engineering: [] })
  assert.deepEqual(groups[0].allowed_groups, [], 'Server confirmed ACL revocation')
  assert.equal(await page.getByRole('switch', { name: 'Intra-group access' }).isChecked(), false)
  await oldAcl.release(); await refreshSettled(); await screenshot('race-acl-after-old-get')
  assert.deepEqual(groups[0].allowed_groups, [], 'Delayed GET did not change the server')
  assert.equal(await page.getByRole('switch', { name: 'Intra-group access' }).isChecked(), false, 'Delayed pre-mutation GET must not restore revoked ACL access')
  await screenshot('race-acl-revoked')
  console.log('PASS: delayed old GET cannot overwrite confirmed ACL revocation; server=[], UI=off, refresh settled')
  await page.getByRole('switch', { name: 'Intra-group access' }).check(); await save({ engineering: ['engineering'] }, { expireToast: true })
  console.log('PASS: ACL save confirmed by its PUT response, server ACL, and clean persistent graph after the transient toast expires; held PUT locks editing')
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
  const releaseGroupCreate = await staleRefresh('/api/groups')
  await page.getByRole('button', { name: 'New group', exact: true }).click(); assert.ok(await page.getByRole('dialog').getByLabel('Name', { exact: true }).evaluate(el => document.activeElement === el), 'New group focuses its name'); await page.getByRole('dialog').getByLabel('Name', { exact: true }).fill('Research'); await page.getByRole('button', { name: 'Create', exact: true }).click(); await node('new-group').waitFor()
  await releaseGroupCreate(); assert.equal(await node('new-group').count(), 1, 'Old groups GET cannot drop a confirmed creation')
  const releaseGroupDelete = await staleRefresh('/api/groups')
  await node('new-group').click(); await page.getByRole('button', { name: 'Delete group', exact: true }).click(); await poll(async () => await node('new-group').count() === 0, 'Group deleted')
  await releaseGroupDelete(); assert.equal(await node('new-group').count(), 0, 'Old groups GET cannot resurrect a deleted group')
  await english(); await screenshot('groups-connected')
  await page.setViewportSize({ width: 390, height: 844 }); await poll(groupsVisible, 'Canvas fits after resizing'); await noOverflow()
  await page.setViewportSize({ width: 1440, height: 1000 }); await poll(groupsVisible, 'Canvas fits after expanding')
  await navigate('Peers')
  const peerBoxes = await page.locator('.peer-card').evaluateAll(cards => cards.map(card => { const b = card.getBoundingClientRect(); return { width: b.width, height: b.height, x: b.x, y: b.y } }))
  assert.ok(peerBoxes.every(b => b.height < 120 && b.width > b.height * 3), 'Peers use compact horizontal rows')
  assert.ok(peerBoxes[1].y > peerBoxes[0].y && peerBoxes[1].x === peerBoxes[0].x, 'Peer rows form a vertical list')
  await page.getByLabel('Search peers').fill('10.77.0.2'); assert.equal(await page.locator('.peer-card').count(), 1); await page.getByLabel('Clear search').click()
  const releasePeerMove = await staleRefresh('/api/peers')
  await page.getByLabel('Group for MacBook Pro').selectOption('operations'); await page.getByText('Group updated', { exact: true }).waitFor()
  await releasePeerMove(); assert.equal(peers[0].group_id, 'operations'); assert.equal(await page.getByLabel('Group for MacBook Pro').inputValue(), 'operations', 'Old peers GET cannot undo a confirmed move')
  const releasePeerCreate = await staleRefresh('/api/peers')
  await page.getByRole('button', { name: 'New peer', exact: true }).click(); await page.getByRole('dialog').getByLabel('Name', { exact: true }).fill('Test laptop'); await page.getByRole('button', { name: 'Create', exact: true }).click(); await page.getByRole('heading', { name: 'Peer ready' }).waitFor(); await english()
  await releasePeerCreate(); assert.equal(await page.locator('.peer-card').count(), 3, 'Old peers GET cannot drop a confirmed creation')
  const download = page.waitForEvent('download'); await page.getByRole('button', { name: 'Download', exact: true }).click(); assert.equal((await download).suggestedFilename(), 'Test-laptop.conf')
  await page.getByRole('button', { name: 'Saved. Close.' }).click()
  const releasePeerDelete = await staleRefresh('/api/peers')
  await page.getByLabel('Delete peer Test laptop').click(); await poll(async () => peers.length === 2 && await page.locator('.peer-card').count() === 2, 'Peer deleted')
  await releasePeerDelete(); assert.equal(await page.locator('.peer-card').count(), 2, 'Old peers GET cannot resurrect a deleted peer'); await screenshot('peers')
  await navigate('Forwards')
  const releaseForwardCreate = await staleRefresh('/api/forwards')
  await page.getByRole('button', { name: 'New forward', exact: true }).click(); await page.getByRole('dialog').getByLabel('Name', { exact: true }).fill('Test service'); await page.getByRole('dialog').getByLabel('Port', { exact: true }).fill('8080'); await page.getByRole('alert').waitFor(); assert.ok(await page.getByRole('button', { name: 'Create', exact: true }).isDisabled())
  await page.getByRole('dialog').getByLabel('Protocol', { exact: true }).selectOption('udp'); await page.getByRole('dialog').getByLabel('Engineering', { exact: false }).check(); await page.getByRole('button', { name: 'Create', exact: true }).click(); await poll(async () => forwards.length === 2 && await page.getByRole('dialog').count() === 0 && await page.locator('.forward-card').count() === 2, 'Forward created'); await english(); await screenshot('forwards')
  await releaseForwardCreate(); assert.equal(await page.locator('.forward-card').count(), 2, 'Old forwards GET cannot drop a confirmed creation')
  const releaseForwardDelete = await staleRefresh('/api/forwards')
  await page.getByLabel('Delete forward Test service').click(); await page.getByText('Forward deleted', { exact: true }).waitFor()
  await releaseForwardDelete(); assert.equal(forwards.length, 1); assert.equal(await page.locator('.forward-card').count(), 1, 'Old forwards GET cannot resurrect a deleted forward')
  console.log('PASS: delayed groups/peers/forwards reads preserve confirmed create, move, and delete mutations; busy settles')
  await navigate('Settings'); await page.getByLabel('Endpoint', { exact: true }).fill('vpn2.example.com:51820'); await page.getByRole('button', { name: 'Save changes', exact: true }).click(); await poll(async () => settings.endpoint === 'vpn2.example.com:51820' && await page.getByRole('button', { name: 'Save changes', exact: true }).count() === 1, 'Defaults saved'); await english(); await screenshot('settings')
  // Settings errors use the backend's plain-text response contract too.
  failSettings = true; await page.getByLabel('Endpoint', { exact: true }).fill('vpn3.example.com:51820'); await page.getByRole('button', { name: 'Save changes', exact: true }).click(); await page.getByRole('alert').filter({ hasText: 'setup required' }).waitFor(); failSettings = false
  // The previous session's refresh must not clear the new session's busy state.
  const oldSessionLoad = holdNext('GET', '/api/groups')
  await refreshSettled(); await refresh.click(); await oldSessionLoad.requested
  await page.getByRole('button', { name: 'Administrator Sign out' }).click(); await page.getByLabel('Access token').waitFor()
  const newSessionLoad = holdNext('GET', '/api/groups')
  await login(); await newSessionLoad.requested
  await oldSessionLoad.release(); await flushFrames()
  assert.ok(await refresh.isDisabled(), 'An old session response cannot clear the new refresh spinner')
  await newSessionLoad.release(); await refreshSettled(); await navigate('Settings')
  console.log('PASS: old-session refresh does not clear new-session busy state')
  // A settings response from the signed-out session must never update a newer session.
  for (const failed of [false, true]) {
    const oldSettings = holdNext('PUT', '/api/settings')
    failSettings = failed
    await page.getByLabel('Endpoint', { exact: true }).fill('old-session.example.com:51820')
    await page.getByRole('button', { name: 'Save changes', exact: true }).click(); await oldSettings.requested
    await page.getByRole('button', { name: 'Administrator Sign out' }).click(); await page.getByLabel('Access token').waitFor()
    assert.equal(await page.getByLabel('Access token').inputValue(), '')
    failSettings = false; settings = { ...settings, endpoint: 'new-session.example.com:51820', persistent_keepalive: 30 }
    await login(); await refreshSettled(); await navigate('Settings')
    assert.equal(await page.getByLabel('Endpoint', { exact: true }).inputValue(), settings.endpoint)
    await oldSettings.release(); await flushFrames(); await screenshot(`race-settings-${failed ? 'failure' : 'success'}-after-old-response`)
    assert.equal(settings.endpoint, 'new-session.example.com:51820', 'Server fixture retains the newer defaults')
    assert.equal(await page.getByLabel('Endpoint', { exact: true }).inputValue(), 'new-session.example.com:51820', 'Late Settings response must not overwrite the new session')
    assert.equal(await page.getByLabel('Keepalive', { exact: false }).inputValue(), '30')
    assert.equal(await page.getByText('Settings saved', { exact: true }).count(), 0, 'No stale Settings success toast')
    assert.equal(await page.getByRole('alert').count(), 0, 'No stale Settings error')
  }
  await screenshot('race-settings-new-session')
  console.log('PASS: late Settings success and failure after sign-out/re-login cannot change new-session defaults, toast, or errors')
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
  await save({ engineering: ['engineering'] }); await connect('operations', 'engineering'); await poll(async () => await edge().count() === 1 && (await edge().getAttribute('aria-label')).includes('Operations'), 'Incoming Operations → Engineering ACL is visible'); await save({ engineering: ['engineering', 'operations'], operations: ['engineering'] })
  // Both Engineering peers were moved/deleted above, so backend group-deletion restrictions permit the request.
  assert.deepEqual(groups.find(g => g.id === 'operations').allowed_groups, ['engineering'])
  const releaseGroupCascade = await staleRefresh('/api/forwards')
  await node('engineering').click(); await page.getByRole('button', { name: 'Delete group', exact: true }).click()
  await poll(async () => !groups.some(g => g.id === 'engineering') && !forwards[0].allowed_group_ids.includes('engineering') && await node('engineering').count() === 0 && await edge().count() === 0, 'Group deletion cascades allowlists')
  assert.deepEqual(groups.find(g => g.id === 'operations').allowed_groups, [], 'Server cascade removes incoming ACL reference')
  assert.equal(await page.locator('.react-flow__edge').count(), 0, 'Local policy graph drops the incoming ACL edge')
  await releaseGroupCascade()
  await navigate('Forwards'); assert.equal(await page.locator('.allow-chips').innerText(), 'No access', 'Old forwards GET cannot restore a deleted group allowlist')
  // A peer deletion cascades its dependent forwards in both server fixtures and UI state, freeing its port.
  await navigate('Peers')
  const releasePeerCascade = await staleRefresh('/api/forwards')
  await page.getByLabel('Delete peer Build server').click(); await poll(async () => peers.length === 1 && forwards.length === 0 && await page.locator('.peer-card').count() === 1, 'Peer deletion cascades target forwards')
  await releasePeerCascade()
  await navigate('Forwards'); assert.equal(await page.locator('.forward-card').count(), 0)
  console.log('PASS: delayed forwards reads cannot undo group allowlist or peer-target deletion cascades')
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
  assert.equal(heldResponses.size, 0, 'All deterministic response barriers were exercised')
  assert.ok(mutations.every(m => m.path.startsWith('/api/')), 'Existing API paths only')
  console.log('PASS: group details closing, compact peer rows, graph direction, drag, persistence, deletion, self-access, partial saves, recovery, CRUD, English, mobile, setup, session reset, and stale-response regressions')
} catch (error) {
  if (page && screenshots) {
    try {
      await mkdir(screenshots, { recursive: true })
      await page.screenshot({ path: resolve(screenshots, 'failure.png'), fullPage: true })
      await writeFile(resolve(screenshots, 'failure.txt'), `${error.stack ?? error}\n\n${await page.locator('body').innerText()}`)
      await page.context().tracing.stop({ path: resolve(screenshots, 'failure-trace.zip') })
    } catch (diagnosticError) { console.error('Unable to capture UI failure artifacts:', diagnosticError) }
  }
  throw error
} finally {
  await browser?.close()
  await new Promise(r => server.close(r))
}
