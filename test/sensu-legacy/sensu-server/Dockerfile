FROM sensu-legacy
RUN /opt/sensu/bin/sensu-install -p disk-checks 
RUN echo '{"client": { "name": "sensu-server", "instance_id": "'i-$(head -c 64 /dev/urandom | sha256sum | cut -c1-17)'", "address": "127.0.0.1", "subscriptions": ["default", "server"] }}' > /etc/sensu/conf.d/client.json
RUN echo '{"checks": { "disk-status": { "command": "check-disk-usage.rb", "standalone": true, "interval": 60 } } }' > /etc/sensu/conf.d/checks.json
CMD /opt/sensu/bin/sensu-client & /opt/sensu/bin/sensu-server
