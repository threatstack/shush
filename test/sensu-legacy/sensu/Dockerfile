FROM ubuntu
RUN apt-get update && apt-get install -y curl gnupg2 build-essential
RUN curl -s https://sensu.global.ssl.fastly.net/apt/pubkey.gpg | apt-key add -
RUN echo "deb https://sensu.global.ssl.fastly.net/apt bionic main" > /etc/apt/sources.list.d/sensu.list
RUN apt-get update
RUN apt-get install sensu
RUN echo '{"redis": { "host": "redis", "password": "testpass" } }' > /etc/sensu/conf.d/redis.json
RUN echo '{"transport": { "name": "redis", "reconnect_on_error": true } }' > /etc/sensu/conf.d/transport.json
