FROM ubuntu
RUN apt-get update
RUN apt-get install -y curl gnupg2
RUN curl -s https://packagecloud.io/install/repositories/sensu/stable/script.deb.sh | bash
RUN apt-get install -y sensu-go-backend sensu-go-cli
