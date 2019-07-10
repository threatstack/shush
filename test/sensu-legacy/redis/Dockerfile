FROM ubuntu
RUN apt-get update
RUN apt-get install -y redis-server
RUN cat /etc/redis/redis.conf | grep -v "bind 127.0.0.1 ::1" > /etc/redis/redis.conf.bind
RUN cat /etc/redis/redis.conf.bind | grep -v "protected-mode yes" > /etc/redis/redis.conf.protec
RUN cat /etc/redis/redis.conf.protec | grep -v "daemonize yes" > /etc/redis/redis.conf.final
RUN echo "bind 0.0.0.0" >> /etc/redis/redis.conf.final
RUN echo "protected-mode no" >> /etc/redis/redis.conf.final
RUN echo "daemonize no" >> /etc/redis/redis.conf.final
RUN echo "requirepass testpass" >> /etc/redis/redis.conf.final
RUN mv /etc/redis/redis.conf.final /etc/redis/redis.conf
CMD redis-server /etc/redis/redis.conf --loglevel verbose
