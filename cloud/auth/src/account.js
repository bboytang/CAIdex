'use strict';
let session;
const byId = id => document.getElementById(id);
const show = message => { byId('status').textContent = message; };
const decode = text => Uint8Array.from(atob(text.replace(/-/g, '+').replace(/_/g, '/')), c => c.charCodeAt(0));
const encode = value => btoa(String.fromCharCode(...new Uint8Array(value))).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
async function request(action, body) {
  const headers = {'Content-Type': 'application/json'};
  if (session) headers.Authorization = `Bearer ${session.access_token}`;
  const response = await fetch(`/v1/auth/${action}`, {method: 'POST', headers, body: JSON.stringify(body), cache: 'no-store', redirect: 'error'});
  const result = await response.json();
  if (!response.ok) throw new Error(result.error || 'Authentication refused');
  return result;
}
async function login(passkey) {
  try {
    if (passkey) {
      const begin = await request('passkey-login', {email: byId('email').value});
      const publicKey = begin.options.publicKey;
      publicKey.challenge = decode(publicKey.challenge);
      publicKey.allowCredentials = publicKey.allowCredentials.map(key => ({...key, id: decode(key.id)}));
      const key = await navigator.credentials.get({publicKey});
      session = await request('passkey-login-finish', {request: begin.request, credential: {
        id: key.id, rawId: encode(key.rawId), type: key.type,
        response: {authenticatorData: encode(key.response.authenticatorData), clientDataJSON: encode(key.response.clientDataJSON), signature: encode(key.response.signature), userHandle: key.response.userHandle ? encode(key.response.userHandle) : null},
        extensions: key.getClientExtensionResults()
      }, platform: 'browser-authorization'});
    } else {
      const password = byId('password').value;
      byId('password').value = '';
      session = await request('login', {email: byId('email').value, password, platform: 'browser-authorization'});
    }
    show('Existing account authenticated. Confirm only your own CLI request.');
    byId('approve').disabled = false;
    byId('deny').disabled = false;
  } catch (error) { show(error.message); }
}
async function approve(approved) {
  byId('approve').disabled = true;
  byId('deny').disabled = true;
  try {
    const context = new URL(location.href).searchParams.get('request');
    const result = await request('approve', {...(context ? {request: context} : {user_code: byId('code').value}), approve: approved});
    show(result.status);
    if (result.redirect_uri) location.assign(result.redirect_uri);
  } catch (error) { show(error.message); }
}
byId('login').addEventListener('click', () => login(false));
byId('passkey').addEventListener('click', () => login(true));
byId('approve').addEventListener('click', () => approve(true));
byId('deny').addEventListener('click', () => approve(false));
