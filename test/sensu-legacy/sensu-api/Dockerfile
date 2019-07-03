FROM sensu-legacy
RUN /opt/sensu/bin/sensu-install -p http
RUN echo '{"client": { "name": "sensu-api", "instance_id": "'i-$(head -c 64 /dev/urandom | sha256sum | cut -c1-17)'", "address": "127.0.0.1", "subscriptions": ["default", "api"] }}' > /etc/sensu/conf.d/client.json
RUN echo '{"checks": { "http-status": { "command": "check-http.rb -u localhost:4567", "standalone": true, "interval": 60 } } }' > /etc/sensu/conf.d/checks.json
CMD /opt/sensu/bin/sensu-client & /opt/sensu/bin/sensu-api
