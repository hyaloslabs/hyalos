import json, sys
for l in open('run/broker.log'):
    o = json.loads(l)
    if 'url' in o:
        auth = o['hdr'].get('authorization') or o['hdr'].get('x-api-key')
        print(o['id'], o['method'], o['url'], '| model=', o.get('model'), 'stream=', o.get('stream'), 'tools=', o.get('tools'), '| cred=', auth)
    elif 'submit' in o:
        print('  SUBMIT', o['submit'])
    elif o.get('unhandled'):
        print('  (unhandled -> 404)', o['id'])
