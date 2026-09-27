from http.server import BaseHTTPRequestHandler, HTTPServer
import json
class Handler(BaseHTTPRequestHandler):
 def do_POST(self):
  body=self.rfile.read(int(self.headers.get('Content-Length',0))).decode()
  data=json.dumps({'path':self.path,'headers':dict(self.headers),'body':json.loads(body) if body else None},indent=2).encode()
  self.send_response(200)
  self.send_header('Content-Type','application/json')
  self.send_header('Content-Length',str(len(data)))
  self.end_headers()
  self.wfile.write(data)
HTTPServer(('127.0.0.1',18765),Handler).serve_forever()
