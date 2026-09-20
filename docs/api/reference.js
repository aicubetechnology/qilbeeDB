'use strict';
(async () => {
  const status = document.getElementById('status');
  const items = [];
  function element(tag, text, className) {
    const node = document.createElement(tag);
    if (text !== undefined) node.textContent = text;
    if (className) node.className = className;
    return node;
  }
  function schemaLink(label, schema) {
    if (!schema || !schema.$ref) return null;
    const name = schema.$ref.split('/').pop();
    const link = element('a', label + ': ' + name);
    link.href = '#schema-' + encodeURIComponent(name);
    link.addEventListener('click', () => {
      const target = document.getElementById('schema-' + name);
      if (target) { target.open = true; target.hidden = false; }
    });
    return link;
  }
  try {
    document.getElementById('base-url').textContent = window.location.origin;
    const response = await fetch('/openapi.json', { credentials: 'omit' });
    if (!response.ok) throw new Error('HTTP ' + response.status);
    const spec = await response.json();
    document.getElementById('description').textContent = spec.info.description;
    const operations = document.getElementById('operation-list');
    let count = 0;
    for (const [path, methods] of Object.entries(spec.paths)) {
      for (const [method, operation] of Object.entries(methods)) {
        if (!['get', 'post', 'put', 'delete', 'patch', 'head', 'options'].includes(method)) continue;
        const card = element('article', undefined, 'endpoint');
        card.id = 'operation-' + operation.operationId;
        const heading = element('div');
        heading.append(element('span', method.toUpperCase(), 'method ' + method), element('code', path, 'path'));
        card.append(heading, element('h3', operation.summary || operation.operationId), element('p', operation.description || '', 'muted'));
        const links = element('div', undefined, 'links');
        const request = schemaLink('Request', operation.requestBody?.content?.['application/json']?.schema);
        if (request) links.append(request);
        const success = operation.responses?.['200'] || operation.responses?.['201'];
        const result = schemaLink('Response', success?.content?.['application/json']?.schema);
        if (result) links.append(result);
        card.append(links);
        if (operation.parameters?.length) {
          const params = element('details');
          params.append(element('summary', 'Parameters'), element('pre', JSON.stringify(operation.parameters, null, 2)));
          card.append(params);
        }
        card.append(element('p', 'Response statuses: ' + Object.keys(operation.responses || {}).join(', '), 'muted'));
        operations.append(card); items.push(card); count++;
      }
    }
    const schemas = document.getElementById('schema-list');
    for (const [name, schema] of Object.entries(spec.components?.schemas || {})) {
      const card = element('details', undefined, 'schema');
      card.id = 'schema-' + name;
      card.append(element('summary', name), element('pre', JSON.stringify(schema, null, 2)));
      schemas.append(card); items.push(card);
    }
    document.getElementById('filter').addEventListener('input', event => {
      const query = event.target.value.trim().toLowerCase();
      for (const card of items) card.hidden = !card.textContent.toLowerCase().includes(query);
    });
    status.textContent = 'Version ' + spec.info.version + ' · ' + count + ' operations · OpenAPI ' + spec.openapi;
  } catch (error) {
    status.classList.add('error');
    status.textContent = 'Could not load the API contract: ' + error.message + '. Check service health and reload.';
  }
})();
