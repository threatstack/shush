FROM sensu-legacy
RUN echo '{"client": { "name": "sensu-server", "instance-id": "'i-$(head -c 64 /dev/urandom | sha256sum | cut -c1-17)'", "address": "127.0.0.1", "subscriptions": ["default", "server"] }}' > /etc/sensu/conf.d/client.json
CMD /opt/sensu/bin/sensu-client & /opt/sensu/bin/sensu-server
